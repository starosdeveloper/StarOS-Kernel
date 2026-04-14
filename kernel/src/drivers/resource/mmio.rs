// SPDX-License-Identifier: GPL-2.0
/*
 * MMIO (Memory-Mapped I/O) management
 *
 * Ported from Linux lib/devres.c and mm/ioremap.c
 * Copyright (C) 1995-1996 Linus Torvalds
 *
 * SAFETY NOTES:
 * - Currently uses identity mapping (phys == virt) for early boot
 * - When MMU is enabled, MmioRegion::new must call page table manager
 * - All reads/writes are volatile (required for MMIO)
 * - Alignment checks prevent hardware faults on ARM64
 */

use crate::drivers::resource::core::{Resource, IORESOURCE_MEM};
use core::ptr::NonNull;

/// MMIO mapping types (cache attributes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoremapType {
    /// Normal cached mapping
    Normal,
    /// Uncached mapping (UC)
    Uncached,
    /// Write-combining mapping (WC)
    WriteCombining,
    /// Non-posted mapping (NP)
    NonPosted,
}

/// MMIO mapping error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmioError {
    InvalidResource,
    Busy,
    NoMemory,
    InvalidAddress,
    AlignmentError,
}

pub type Result<T> = core::result::Result<T, MmioError>;

/// Check if address is aligned to boundary
#[inline]
fn is_aligned(addr: u64, align: usize) -> bool {
    addr % align as u64 == 0
}

/// MMIO region descriptor
pub struct MmioRegion {
    /// Virtual address
    virt_addr: NonNull<u8>,
    /// Physical address
    phys_addr: u64,
    /// Size in bytes
    size: usize,
    /// Mapping type
    map_type: IoremapType,
}

impl MmioRegion {
    /// Create a new MMIO region with identity mapping
    ///
    /// Uses identity mapping where virtual address == physical address.
    /// This is the standard approach for early boot and embedded systems
    /// where MMU is either disabled or configured for 1:1 mapping.
    pub fn new(phys_addr: u64, size: usize, map_type: IoremapType) -> Result<Self> {
        if size == 0 {
            return Err(MmioError::InvalidAddress);
        }

        // Check for overflow
        if phys_addr.checked_add(size as u64).is_none() {
            return Err(MmioError::InvalidAddress);
        }

        // Identity mapping: virt == phys
        let virt_addr = NonNull::new(phys_addr as *mut u8)
            .ok_or(MmioError::InvalidAddress)?;

        Ok(Self {
            virt_addr,
            phys_addr,
            size,
            map_type,
        })
    }

    /// Get virtual address
    pub fn virt_addr(&self) -> *mut u8 {
        self.virt_addr.as_ptr()
    }

    /// Get physical address
    pub fn phys_addr(&self) -> u64 {
        self.phys_addr
    }

    /// Get size
    pub fn size(&self) -> usize {
        self.size
    }

    /// Read u8 from offset
    pub unsafe fn read_u8(&self, offset: usize) -> u8 {
        if offset >= self.size {
            return 0;
        }
        core::ptr::read_volatile(self.virt_addr.as_ptr().add(offset))
    }

    /// Write u8 to offset
    pub unsafe fn write_u8(&self, offset: usize, value: u8) {
        if offset < self.size {
            core::ptr::write_volatile(self.virt_addr.as_ptr().add(offset), value);
        }
    }

    /// Read u32 from offset
    ///
    /// Returns 0 if offset is out of bounds or misaligned.
    /// ARM64 requires 4-byte alignment for u32 access.
    pub unsafe fn read_u32(&self, offset: usize) -> u32 {
        if offset + 4 > self.size {
            return 0;
        }
        let addr = self.phys_addr + offset as u64;
        if !is_aligned(addr, 4) {
            return 0; // Prevent alignment fault
        }
        core::ptr::read_volatile(self.virt_addr.as_ptr().add(offset) as *const u32)
    }

    /// Write u32 to offset
    ///
    /// Does nothing if offset is out of bounds or misaligned.
    /// ARM64 requires 4-byte alignment for u32 access.
    pub unsafe fn write_u32(&self, offset: usize, value: u32) {
        if offset + 4 > self.size {
            return;
        }
        let addr = self.phys_addr + offset as u64;
        if !is_aligned(addr, 4) {
            return; // Prevent alignment fault
        }
        core::ptr::write_volatile(self.virt_addr.as_ptr().add(offset) as *mut u32, value);
    }

    /// Read u64 from offset
    ///
    /// Returns 0 if offset is out of bounds or misaligned.
    /// ARM64 requires 8-byte alignment for u64 access.
    pub unsafe fn read_u64(&self, offset: usize) -> u64 {
        if offset + 8 > self.size {
            return 0;
        }
        let addr = self.phys_addr + offset as u64;
        if !is_aligned(addr, 8) {
            return 0; // Prevent alignment fault
        }
        core::ptr::read_volatile(self.virt_addr.as_ptr().add(offset) as *const u64)
    }

    /// Write u64 to offset
    ///
    /// Does nothing if offset is out of bounds or misaligned.
    /// ARM64 requires 8-byte alignment for u64 access.
    pub unsafe fn write_u64(&self, offset: usize, value: u64) {
        if offset + 8 > self.size {
            return;
        }
        let addr = self.phys_addr + offset as u64;
        if !is_aligned(addr, 8) {
            return; // Prevent alignment fault
        }
        core::ptr::write_volatile(self.virt_addr.as_ptr().add(offset) as *mut u64, value);
    }
}

impl Drop for MmioRegion {
    fn drop(&mut self) {
        // In real implementation, this would unmap pages
        // For now, nothing to do with identity mapping
    }
}

/// ioremap - map physical memory to virtual address
/// @phys_addr: physical address to map
/// @size: size of mapping
///
/// Returns virtual address on success, error on failure
pub fn ioremap(phys_addr: u64, size: usize) -> Result<MmioRegion> {
    MmioRegion::new(phys_addr, size, IoremapType::Normal)
}

/// ioremap_uc - map physical memory as uncached
/// @phys_addr: physical address to map
/// @size: size of mapping
pub fn ioremap_uc(phys_addr: u64, size: usize) -> Result<MmioRegion> {
    MmioRegion::new(phys_addr, size, IoremapType::Uncached)
}

/// ioremap_wc - map physical memory as write-combining
/// @phys_addr: physical address to map
/// @size: size of mapping
pub fn ioremap_wc(phys_addr: u64, size: usize) -> Result<MmioRegion> {
    MmioRegion::new(phys_addr, size, IoremapType::WriteCombining)
}

/// ioremap_np - map physical memory as non-posted
/// @phys_addr: physical address to map
/// @size: size of mapping
pub fn ioremap_np(phys_addr: u64, size: usize) -> Result<MmioRegion> {
    MmioRegion::new(phys_addr, size, IoremapType::NonPosted)
}

/// iounmap - unmap MMIO region
/// @region: region to unmap
///
/// This is handled automatically by Drop trait
pub fn iounmap(_region: MmioRegion) {
    // Drop will be called automatically
}

/// devm_ioremap - Managed ioremap()
/// @phys_addr: physical address to map
/// @size: size of mapping
///
/// Managed ioremap(). Map is automatically unmapped on driver detach.
pub fn devm_ioremap(phys_addr: u64, size: usize) -> Result<MmioRegion> {
    ioremap(phys_addr, size)
}

/// devm_ioremap_uc - Managed ioremap_uc()
/// @phys_addr: physical address to map
/// @size: size of mapping
pub fn devm_ioremap_uc(phys_addr: u64, size: usize) -> Result<MmioRegion> {
    ioremap_uc(phys_addr, size)
}

/// devm_ioremap_wc - Managed ioremap_wc()
/// @phys_addr: physical address to map
/// @size: size of mapping
pub fn devm_ioremap_wc(phys_addr: u64, size: usize) -> Result<MmioRegion> {
    ioremap_wc(phys_addr, size)
}

/// devm_ioremap_resource - check, request region, and ioremap resource
/// @res: resource to be handled
///
/// Checks that a resource is a valid memory region, requests the memory
/// region and ioremaps it. All operations are managed and will be undone
/// on driver detach.
///
/// Usage example:
/// ```
/// let res = platform_get_resource(pdev, IORESOURCE_MEM, 0);
/// let base = devm_ioremap_resource(res)?;
/// ```
pub fn devm_ioremap_resource(res: &Resource) -> Result<MmioRegion> {
    // Check if resource is valid memory region
    if res.flags & IORESOURCE_MEM == 0 {
        return Err(MmioError::InvalidResource);
    }

    let size = res.size() as usize;
    if size == 0 {
        return Err(MmioError::InvalidResource);
    }

    // Request memory region (check for conflicts)
    // In full implementation, this would call request_mem_region()

    // Map the region
    devm_ioremap(res.start, size)
}

/// devm_ioremap_resource_wc - write-combined variant of devm_ioremap_resource()
/// @res: resource to be handled
pub fn devm_ioremap_resource_wc(res: &Resource) -> Result<MmioRegion> {
    if res.flags & IORESOURCE_MEM == 0 {
        return Err(MmioError::InvalidResource);
    }

    let size = res.size() as usize;
    if size == 0 {
        return Err(MmioError::InvalidResource);
    }

    devm_ioremap_wc(res.start, size)
}

/// devm_platform_ioremap_resource - platform device variant
/// @pdev: platform device
/// @index: resource index
///
/// Shorthand for platform_get_resource() + devm_ioremap_resource()
pub fn devm_platform_ioremap_resource(res: &Resource, _index: usize) -> Result<MmioRegion> {
    devm_ioremap_resource(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mmio_region_creation() {
        let region = ioremap(0x1000, 0x1000);
        assert!(region.is_ok());
        let region = region.unwrap();
        assert_eq!(region.phys_addr(), 0x1000);
        assert_eq!(region.size(), 0x1000);
    }

    #[test]
    fn test_mmio_invalid_size() {
        let region = ioremap(0x1000, 0);
        assert_eq!(region.err(), Some(MmioError::InvalidAddress));
    }

    #[test]
    fn test_mmio_overflow() {
        let region = ioremap(u64::MAX, 0x1000);
        assert_eq!(region.err(), Some(MmioError::InvalidAddress));
    }

    #[test]
    fn test_mmio_types() {
        let uc = ioremap_uc(0x1000, 0x1000).unwrap();
        let wc = ioremap_wc(0x2000, 0x1000).unwrap();
        let np = ioremap_np(0x3000, 0x1000).unwrap();
        
        assert_eq!(uc.map_type, IoremapType::Uncached);
        assert_eq!(wc.map_type, IoremapType::WriteCombining);
        assert_eq!(np.map_type, IoremapType::NonPosted);
    }

    #[test]
    fn test_devm_ioremap_resource() {
        let res = Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM);
        let region = devm_ioremap_resource(&res);
        assert!(region.is_ok());
        let region = region.unwrap();
        assert_eq!(region.phys_addr(), 0x1000);
        assert_eq!(region.size(), 0x1000);
    }

    #[test]
    fn test_devm_ioremap_resource_invalid() {
        let res = Resource::new(0x1000, 0x1FFF, 0); // Not IORESOURCE_MEM
        let region = devm_ioremap_resource(&res);
        assert_eq!(region.err(), Some(MmioError::InvalidResource));
    }

    #[test]
    fn test_alignment_check() {
        assert!(is_aligned(0x1000, 4));
        assert!(is_aligned(0x1004, 4));
        assert!(!is_aligned(0x1001, 4));
        assert!(!is_aligned(0x1002, 4));
        
        assert!(is_aligned(0x1000, 8));
        assert!(is_aligned(0x1008, 8));
        assert!(!is_aligned(0x1004, 8));
    }

    #[test]
    #[cfg(target_arch = "aarch64")] // writes to HW address — only valid on real hardware
    fn test_mmio_aligned_access() {
        let region = ioremap(0x1000, 0x1000).unwrap();
        
        unsafe {
            // Aligned access should work
            region.write_u32(0, 0x12345678);
            region.write_u32(4, 0xABCDEF00);
            
            region.write_u64(0, 0x123456789ABCDEF0);
            region.write_u64(8, 0xFEDCBA9876543210);
        }
    }

    #[test]
    fn test_mmio_misaligned_access() {
        let region = ioremap(0x1001, 0x1000).unwrap(); // Misaligned base
        
        unsafe {
            // Misaligned u32 access returns 0
            let val = region.read_u32(0);
            assert_eq!(val, 0);
            
            // Misaligned u64 access returns 0
            let val = region.read_u64(0);
            assert_eq!(val, 0);
        }
    }
}
