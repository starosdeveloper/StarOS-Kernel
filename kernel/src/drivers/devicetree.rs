//! Device Tree parser
//!
//! Parses Flattened Device Tree (FDT) to discover hardware

#[cfg(not(feature = "std"))]
use core::slice;
#[cfg(feature = "std")]
use std::slice;

use crate::error::KernelError;

/// FDT magic number
const FDT_MAGIC: u32 = 0xd00dfeed;

/// FDT header
#[repr(C)]
struct FdtHeader {
    magic: u32,
    totalsize: u32,
    off_dt_struct: u32,
    off_dt_strings: u32,
    off_mem_rsvmap: u32,
    version: u32,
    last_comp_version: u32,
    boot_cpuid_phys: u32,
    size_dt_strings: u32,
    size_dt_struct: u32,
}

/// Device tree node
#[derive(Debug, Clone)]
pub struct DeviceNode {
    pub name: &'static str,
    pub compatible: Option<&'static str>,
    pub reg: Option<(u64, u64)>, // (address, size)
    pub interrupts: Option<u32>,
}

/// Device tree parser
pub struct DeviceTree {
    base: usize,
    size: usize,
}

impl DeviceTree {
    /// Create device tree parser from address
    /// 
    /// # Safety
    /// base must point to valid FDT blob
    pub unsafe fn from_ptr(base: usize) -> Result<Self, KernelError> {
        let header = &*(base as *const FdtHeader);
        
        if u32::from_be(header.magic) != FDT_MAGIC {
            return Err(KernelError::InvalidParameter("Invalid FDT magic"));
        }

        let size = u32::from_be(header.totalsize) as usize;

        Ok(Self { base, size })
    }

    /// Find node by compatible string
    pub fn find_compatible(&self, compatible: &str) -> Option<DeviceNode> {
        // Simplified parser - in real implementation would walk FDT structure
        // For now, return None
        let _ = compatible;
        None
    }

    /// Find node by path
    pub fn find_node(&self, path: &str) -> Option<DeviceNode> {
        let _ = path;
        None
    }

    /// Get memory regions
    pub fn memory_regions(&self) -> &[(u64, u64)] {
        // Would parse /memory node
        &[]
    }

    /// Get chosen node (bootargs, initrd, etc)
    pub fn chosen(&self) -> Option<DeviceNode> {
        None
    }

    /// Get base address
    pub fn base(&self) -> usize {
        self.base
    }

    /// Get size
    pub fn size(&self) -> usize {
        self.size
    }
}

unsafe impl Send for DeviceTree {}
unsafe impl Sync for DeviceTree {}

/// Device discovery from device tree
pub struct DeviceDiscovery {
    uart_base: Option<usize>,
    timer_freq: Option<u32>,
    gic_dist_base: Option<usize>,
    gic_cpu_base: Option<usize>,
}

impl DeviceDiscovery {
    pub const fn new() -> Self {
        Self {
            uart_base: None,
            timer_freq: None,
            gic_dist_base: None,
            gic_cpu_base: None,
        }
    }

    /// Discover devices from device tree
    pub fn discover(&mut self, dt: &DeviceTree) -> Result<(), KernelError> {
        // Find UART
        if let Some(node) = dt.find_compatible("arm,pl011") {
            if let Some((addr, _size)) = node.reg {
                self.uart_base = Some(addr as usize);
            }
        }

        // Find timer frequency
        if let Some(node) = dt.find_node("/timer") {
            // Would parse clock-frequency property
            self.timer_freq = Some(24_000_000); // Default 24MHz
        }

        // Find GIC
        if let Some(node) = dt.find_compatible("arm,gic-v3") {
            if let Some((addr, _size)) = node.reg {
                self.gic_dist_base = Some(addr as usize);
                self.gic_cpu_base = Some((addr + 0x10000) as usize);
            }
        }

        Ok(())
    }

    pub fn uart_base(&self) -> Option<usize> {
        self.uart_base
    }

    pub fn timer_freq(&self) -> Option<u32> {
        self.timer_freq
    }

    pub fn gic_dist_base(&self) -> Option<usize> {
        self.gic_dist_base
    }

    pub fn gic_cpu_base(&self) -> Option<usize> {
        self.gic_cpu_base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_discovery() {
        let mut discovery = DeviceDiscovery::new();
        assert!(discovery.uart_base().is_none());
        assert!(discovery.timer_freq().is_none());
    }

    #[test]
    fn test_fdt_magic() {
        assert_eq!(FDT_MAGIC, 0xd00dfeed);
    }
}
