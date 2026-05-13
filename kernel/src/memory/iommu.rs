//! ARM SMMU (System MMU) support for IOMMU device isolation.

use core::sync::atomic::{AtomicBool, Ordering};

const MAX_DEVICES: usize = 32;
const MAX_MAPPINGS_PER_DEVICE: usize = 16;

/// Protection flags for IOMMU mappings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IommuProt(pub u32);

impl IommuProt {
    pub const READ: Self = Self(1);
    pub const WRITE: Self = Self(2);
    pub const RW: Self = Self(3);
}

/// SMMU Stream Table Entry — associates a device stream with a translation context.
#[derive(Clone, Copy, Debug)]
pub struct StreamTableEntry {
    pub valid: bool,
    pub device_id: u16,
    pub domain_id: u16,
    pub s1_context_ptr: u64,
}

/// A single IOVA-to-physical mapping.
#[derive(Clone, Copy, Debug)]
struct IovaMapping {
    iova: u64,
    phys_addr: u64,
    size: usize,
    prot: u32,
}

/// Per-device IOMMU state.
struct DeviceContext {
    attached: bool,
    domain_id: u16,
    mappings: [Option<IovaMapping>; MAX_MAPPINGS_PER_DEVICE],
}

impl DeviceContext {
    const fn new() -> Self {
        Self {
            attached: false,
            domain_id: 0,
            mappings: [None; MAX_MAPPINGS_PER_DEVICE],
        }
    }
}

/// Global IOMMU state.
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static mut STREAM_TABLE: [StreamTableEntry; MAX_DEVICES] = [StreamTableEntry {
    valid: false, device_id: 0, domain_id: 0, s1_context_ptr: 0,
}; MAX_DEVICES];
static mut DEVICES: [DeviceContext; MAX_DEVICES] = {
    const INIT: DeviceContext = DeviceContext::new();
    [INIT; MAX_DEVICES]
};

fn check_device_id(device_id: u16) -> Result<usize, &'static str> {
    let idx = device_id as usize;
    if idx >= MAX_DEVICES { Err("device_id out of range") } else { Ok(idx) }
}

/// Attach a device to an IOMMU domain.
pub fn iommu_attach_device(device_id: u16, domain_id: u16) -> Result<(), &'static str> {
    let idx = check_device_id(device_id)?;
    // SAFETY: single-core kernel init, no concurrent access during setup.
    unsafe {
        if DEVICES[idx].attached {
            return Err("device already attached");
        }
        DEVICES[idx].attached = true;
        DEVICES[idx].domain_id = domain_id;
        STREAM_TABLE[idx] = StreamTableEntry {
            valid: true, device_id, domain_id, s1_context_ptr: 0,
        };
    }
    INITIALIZED.store(true, Ordering::Release);
    Ok(())
}

/// Detach a device from its IOMMU domain.
pub fn iommu_detach_device(device_id: u16) -> Result<(), &'static str> {
    let idx = check_device_id(device_id)?;
    // SAFETY: single-core kernel context.
    unsafe {
        if !DEVICES[idx].attached {
            return Err("device not attached");
        }
        DEVICES[idx].attached = false;
        DEVICES[idx].domain_id = 0;
        DEVICES[idx].mappings = [None; MAX_MAPPINGS_PER_DEVICE];
        STREAM_TABLE[idx].valid = false;
    }
    Ok(())
}

/// Create an IOVA mapping for a device.
pub fn iommu_map(device_id: u16, iova: u64, phys_addr: u64, size: usize, prot: IommuProt) -> Result<(), &'static str> {
    let idx = check_device_id(device_id)?;
    // SAFETY: single-core kernel context.
    unsafe {
        if !DEVICES[idx].attached {
            return Err("device not attached");
        }
        for slot in DEVICES[idx].mappings.iter_mut() {
            if slot.is_none() {
                *slot = Some(IovaMapping { iova, phys_addr, size, prot: prot.0 });
                return Ok(());
            }
        }
    }
    Err("no free mapping slots")
}

/// Remove an IOVA mapping for a device.
pub fn iommu_unmap(device_id: u16, iova: u64, size: usize) -> Result<(), &'static str> {
    let idx = check_device_id(device_id)?;
    // SAFETY: single-core kernel context.
    unsafe {
        if !DEVICES[idx].attached {
            return Err("device not attached");
        }
        for slot in DEVICES[idx].mappings.iter_mut() {
            if let Some(m) = slot {
                if m.iova == iova && m.size == size {
                    *slot = None;
                    return Ok(());
                }
            }
        }
    }
    Err("mapping not found")
}

/// SMMU fault handler stub — called on translation faults.
pub fn iommu_fault_handler(device_id: u16, fault_addr: u64) {
    // Stub: log fault for debugging. In production, signal the owning domain.
    let _ = (device_id, fault_addr);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attach_detach() {
        assert!(iommu_attach_device(0, 1).is_ok());
        assert!(iommu_attach_device(0, 2).is_err()); // already attached
        assert!(iommu_detach_device(0).is_ok());
        assert!(iommu_detach_device(0).is_err()); // not attached
    }

    #[test]
    fn test_map_unmap() {
        iommu_attach_device(1, 1).unwrap();
        assert!(iommu_map(1, 0x1000, 0x8000_0000, 4096, IommuProt::RW).is_ok());
        assert!(iommu_unmap(1, 0x1000, 4096).is_ok());
        assert!(iommu_unmap(1, 0x1000, 4096).is_err()); // already removed
        iommu_detach_device(1).unwrap();
    }

    #[test]
    fn test_map_without_attach() {
        assert!(iommu_map(2, 0x2000, 0x9000_0000, 4096, IommuProt::READ).is_err());
    }

    #[test]
    fn test_invalid_device_id() {
        assert!(iommu_attach_device(32, 0).is_err());
    }

    #[test]
    fn test_fault_handler_does_not_panic() {
        iommu_fault_handler(0, 0xDEAD_BEEF);
    }
}
