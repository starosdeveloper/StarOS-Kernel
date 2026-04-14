// SPDX-License-Identifier: MIT OR Apache-2.0
//! DMA Mapping
//!
//! Ported from Linux: kernel/dma/mapping.c
//! Source lines: ~800 C → ~500 Rust

use crate::drivers::base::Device;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};

/// DMA address type
pub type DmaAddr = u64;

/// DMA data direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDataDirection {
    Bidirectional = 0,
    ToDevice = 1,
    FromDevice = 2,
    None = 3,
}

/// DMA attributes
pub mod dma_attrs {
    pub const WRITE_BARRIER: u64 = 1 << 0;
    pub const WEAK_ORDERING: u64 = 1 << 1;
    pub const WRITE_COMBINE: u64 = 1 << 2;
    pub const NON_CONSISTENT: u64 = 1 << 3;
    pub const NO_KERNEL_MAPPING: u64 = 1 << 4;
    pub const SKIP_CPU_SYNC: u64 = 1 << 5;
}

/// DMA allocation result
#[derive(Debug)]
pub struct DmaAllocation {
    pub vaddr: *mut u8,
    pub dma_handle: DmaAddr,
    pub size: usize,
}

/// Managed DMA resource
struct DmaDevres {
    size: usize,
    vaddr: *mut u8,
    dma_handle: DmaAddr,
    attrs: u64,
}

/// Allocate coherent DMA memory
///
/// Ported from: dma_alloc_attrs()
pub fn dma_alloc_coherent(dev: &Device, size: usize, dma_handle: &mut DmaAddr) -> *mut u8 {
    dma_alloc_attrs(dev, size, dma_handle, 0)
}

/// Allocate DMA memory with attributes
///
/// Ported from: dma_alloc_attrs()
pub fn dma_alloc_attrs(dev: &Device, size: usize, dma_handle: &mut DmaAddr, attrs: u64) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    // Align size to page boundary
    let aligned_size = (size + 4095) & !4095;

    // Allocate memory (simplified - in real implementation would use allocator)
    let vaddr = unsafe {
        alloc::alloc::alloc_zeroed(
            alloc::alloc::Layout::from_size_align_unchecked(aligned_size, 4096)
        )
    };

    if vaddr.is_null() {
        return core::ptr::null_mut();
    }

    // Get DMA address (simplified - would use IOMMU/direct mapping)
    *dma_handle = vaddr as u64;

    vaddr
}

/// Free coherent DMA memory
///
/// Ported from: dma_free_coherent()
pub fn dma_free_coherent(dev: &Device, size: usize, vaddr: *mut u8, dma_handle: DmaAddr) {
    dma_free_attrs(dev, size, vaddr, dma_handle, 0);
}

/// Free DMA memory with attributes
///
/// Ported from: dma_free_attrs()
pub fn dma_free_attrs(dev: &Device, size: usize, vaddr: *mut u8, dma_handle: DmaAddr, attrs: u64) {
    if vaddr.is_null() {
        return;
    }

    let aligned_size = (size + 4095) & !4095;

    unsafe {
        alloc::alloc::dealloc(
            vaddr,
            alloc::alloc::Layout::from_size_align_unchecked(aligned_size, 4096)
        );
    }
}

/// Map single buffer for DMA
///
/// Ported from: dma_map_single()
pub fn dma_map_single(dev: &Device, ptr: *mut u8, size: usize, dir: DmaDataDirection) -> DmaAddr {
    dma_map_single_attrs(dev, ptr, size, dir, 0)
}

/// Map single buffer with attributes
///
/// Ported from: dma_map_single_attrs()
pub fn dma_map_single_attrs(
    dev: &Device,
    ptr: *mut u8,
    size: usize,
    dir: DmaDataDirection,
    attrs: u64,
) -> DmaAddr {
    if ptr.is_null() || size == 0 {
        return 0;
    }

    // Check if device uses IOMMU
    if dev.has_iommu() {
        // IOMMU path: translate virtual -> IOVA (IO Virtual Address)
        // In full implementation:
        // return iommu_map_page(dev, ptr, size, dir);
        ptr as u64
    } else {
        // Direct mapping: virtual address = physical address
        // For Star-X with unified memory, this is valid
        ptr as u64
    }
}

/// Unmap single buffer
///
/// Ported from: dma_unmap_single()
pub fn dma_unmap_single(dev: &Device, addr: DmaAddr, size: usize, dir: DmaDataDirection) {
    dma_unmap_single_attrs(dev, addr, size, dir, 0);
}

/// Unmap single buffer with attributes
///
/// Ported from: dma_unmap_single_attrs()
pub fn dma_unmap_single_attrs(
    dev: &Device,
    addr: DmaAddr,
    size: usize,
    dir: DmaDataDirection,
    attrs: u64,
) {
    if dev.has_iommu() {
        // IOMMU path: unmap IOVA
        // In full implementation:
        // iommu_unmap_page(dev, addr, size);
    }
    // Direct mapping: no-op
}

/// Sync single buffer for CPU
///
/// Ported from: dma_sync_single_for_cpu()
pub fn dma_sync_single_for_cpu(dev: &Device, addr: DmaAddr, size: usize, dir: DmaDataDirection) {
    // Memory barrier: ensure DMA writes are visible to CPU
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // ARM64: Data Memory Barrier - wait for all memory operations
        core::arch::asm!("dmb sy", options(nostack, preserves_flags));
    }
    
    #[cfg(target_arch = "x86_64")]
    unsafe {
        // x86: Memory fence
        core::arch::asm!("mfence", options(nostack, preserves_flags));
    }
    
    // Cache invalidation for non-coherent devices
    if !dev.is_dma_coherent() {
        // Invalidate cache lines to force CPU to read from RAM
        arch_invalidate_cache_range(addr, size);
    }
}

/// Sync single buffer for device
///
/// Ported from: dma_sync_single_for_device()
pub fn dma_sync_single_for_device(dev: &Device, addr: DmaAddr, size: usize, dir: DmaDataDirection) {
    // Cache flush for non-coherent devices
    if !dev.is_dma_coherent() {
        // Flush dirty cache lines to RAM
        arch_flush_cache_range(addr, size);
    }
    
    // Memory barrier: ensure CPU writes reach RAM before DMA starts
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // ARM64: Data Synchronization Barrier - complete all memory operations
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    
    #[cfg(target_arch = "x86_64")]
    unsafe {
        // x86: Store fence
        core::arch::asm!("sfence", options(nostack, preserves_flags));
    }
}

/// Architecture-specific cache invalidation
#[cfg(target_arch = "aarch64")]
fn arch_invalidate_cache_range(addr: DmaAddr, size: usize) {
    let start = addr as usize;
    let end = start + size;
    let cache_line_size = 64; // ARM64 typical cache line
    
    let mut line = start & !(cache_line_size - 1);
    while line < end {
        unsafe {
            core::arch::asm!(
                "dc ivac, {0}",
                in(reg) line,
                options(nostack, preserves_flags)
            );
        }
        line += cache_line_size;
    }
}

/// Architecture-specific cache flush
#[cfg(target_arch = "aarch64")]
fn arch_flush_cache_range(addr: DmaAddr, size: usize) {
    let start = addr as usize;
    let end = start + size;
    let cache_line_size = 64;
    
    let mut line = start & !(cache_line_size - 1);
    while line < end {
        unsafe {
            core::arch::asm!(
                "dc cvac, {0}",
                in(reg) line,
                options(nostack, preserves_flags)
            );
        }
        line += cache_line_size;
    }
}

#[cfg(not(target_arch = "aarch64"))]
fn arch_invalidate_cache_range(_addr: DmaAddr, _size: usize) {}

#[cfg(not(target_arch = "aarch64"))]
fn arch_flush_cache_range(_addr: DmaAddr, _size: usize) {}

/// Managed DMA allocation
///
/// Ported from: dmam_alloc_attrs()
pub fn dmam_alloc_coherent(
    dev: &Device,
    size: usize,
    dma_handle: &mut DmaAddr,
) -> *mut u8 {
    dmam_alloc_attrs(dev, size, dma_handle, 0)
}

/// Managed DMA allocation with attributes
///
/// Ported from: dmam_alloc_attrs()
pub fn dmam_alloc_attrs(
    dev: &Device,
    size: usize,
    dma_handle: &mut DmaAddr,
    attrs: u64,
) -> *mut u8 {
    let vaddr = dma_alloc_attrs(dev, size, dma_handle, attrs);
    
    if !vaddr.is_null() {
        // Register with devres for automatic cleanup
        let resource = DmaDevres {
            size,
            vaddr,
            dma_handle: *dma_handle,
            attrs,
        };
        dev.add_resource(Box::new(resource));
    }
    
    vaddr
}

/// Managed DMA free
///
/// Ported from: dmam_free_coherent()
pub fn dmam_free_coherent(dev: &Device, size: usize, vaddr: *mut u8, dma_handle: DmaAddr) {
    // Remove from devres
    dev.remove_resource(|res| {
        if let Some(dma_res) = res.downcast_ref::<DmaDevres>() {
            dma_res.vaddr == vaddr
        } else {
            false
        }
    });
    
    dma_free_coherent(dev, size, vaddr, dma_handle);
}

/// Devres cleanup callback for DMA allocations
impl Drop for DmaDevres {
    fn drop(&mut self) {
        // Auto-cleanup on device detach
        // In full implementation, would get device reference
        // dma_free_attrs(dev, self.size, self.vaddr, self.dma_handle, self.attrs);
    }
}

/// Get DMA mask
pub fn dma_get_mask(dev: &Device) -> u64 {
    dev.dma_mask.load(Ordering::Acquire)
}

/// Set DMA mask
pub fn dma_set_mask(dev: &Device, mask: u64) -> Result<(), i32> {
    dev.dma_mask.store(mask, Ordering::Release);
    Ok(())
}

/// Set coherent DMA mask
pub fn dma_set_coherent_mask(dev: &Device, mask: u64) -> Result<(), i32> {
    dev.coherent_dma_mask.store(mask, Ordering::Release);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dma_alloc_free_coherent() {
        let dev = Device::mock();
        let mut dma_handle: DmaAddr = 0;
        
        let vaddr = dma_alloc_coherent(&dev, 4096, &mut dma_handle);
        assert!(!vaddr.is_null());
        assert_ne!(dma_handle, 0);
        
        dma_free_coherent(&dev, 4096, vaddr, dma_handle);
    }

    #[test]
    fn test_dma_map_unmap_single() {
        let dev = Device::mock();
        let buffer = vec![0u8; 1024];
        let ptr = buffer.as_ptr() as *mut u8;
        
        let dma_addr = dma_map_single(&dev, ptr, 1024, DmaDataDirection::ToDevice);
        assert_ne!(dma_addr, 0);
        
        dma_unmap_single(&dev, dma_addr, 1024, DmaDataDirection::ToDevice);
    }

    #[test]
    fn test_dma_set_get_mask() {
        let dev = Device::mock();
        
        let result = dma_set_mask(&dev, 0xFFFFFFFF);
        assert!(result.is_ok());
        
        let mask = dma_get_mask(&dev);
        assert_eq!(mask, 0xFFFFFFFF);
    }

    #[test]
    fn test_dma_alloc_attrs() {
        let dev = Device::mock();
        let mut dma_handle: DmaAddr = 0;
        
        let vaddr = dma_alloc_attrs(&dev, 8192, &mut dma_handle, dma_attrs::WRITE_COMBINE);
        assert!(!vaddr.is_null());
        
        dma_free_attrs(&dev, 8192, vaddr, dma_handle, dma_attrs::WRITE_COMBINE);
    }

    #[test]
    fn test_dmam_alloc_free() {
        let dev = Device::mock();
        let mut dma_handle: DmaAddr = 0;
        
        let vaddr = dmam_alloc_coherent(&dev, 4096, &mut dma_handle);
        assert!(!vaddr.is_null());
        
        dmam_free_coherent(&dev, 4096, vaddr, dma_handle);
    }
}
