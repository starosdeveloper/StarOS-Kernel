//! Memory FFI — Kotlin/Native bindings for memory management
//!
//! Exports memory management functionality with C ABI:
//! - Buffer allocation/deallocation
//! - Physical/virtual memory mapping
//! - DMA buffer management
//! - Zero-copy buffer sharing
//! - Memory statistics

use super::types::*;
use core::alloc::Layout;

// Note: Some memory functions are stubs until proper allocator is integrated

// ---------------------------------------------------------------------------
// Memory allocation
// ---------------------------------------------------------------------------

/// Allocate memory buffer
///
/// Returns a pointer to the allocated buffer, or null on failure.
/// The buffer must be freed with `staros_memory_free`.
#[no_mangle]
pub extern "C" fn staros_memory_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    let layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };

    #[cfg(not(feature = "std"))]
    unsafe {
        alloc::alloc::alloc(layout)
    }
    
    #[cfg(feature = "std")]
    unsafe {
        std::alloc::alloc(layout)
    }
}

/// Allocate zeroed memory buffer
#[no_mangle]
pub extern "C" fn staros_memory_alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }

    let layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };

    #[cfg(not(feature = "std"))]
    unsafe {
        alloc::alloc::alloc_zeroed(layout)
    }
    
    #[cfg(feature = "std")]
    unsafe {
        std::alloc::alloc_zeroed(layout)
    }
}

/// Free memory buffer
///
/// # Safety
/// - `ptr` must have been allocated by `staros_memory_alloc` or `staros_memory_alloc_zeroed`
/// - `size` and `align` must match the original allocation
/// - `ptr` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn staros_memory_free(ptr: *mut u8, size: usize, align: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }

    let layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return,
    };

    #[cfg(not(feature = "std"))]
    alloc::alloc::dealloc(ptr, layout);
    
    #[cfg(feature = "std")]
    std::alloc::dealloc(ptr, layout);
}

/// Reallocate memory buffer
///
/// # Safety
/// - `ptr` must have been allocated by `staros_memory_alloc`
/// - `old_size` and `align` must match the original allocation
#[no_mangle]
pub unsafe extern "C" fn staros_memory_realloc(
    ptr: *mut u8,
    old_size: usize,
    new_size: usize,
    align: usize,
) -> *mut u8 {
    if ptr.is_null() {
        return staros_memory_alloc(new_size, align);
    }

    if new_size == 0 {
        staros_memory_free(ptr, old_size, align);
        return core::ptr::null_mut();
    }

    // Prevent excessive allocations (256MB limit)
    const MAX_REALLOC_SIZE: usize = 256 * 1024 * 1024;
    if new_size > MAX_REALLOC_SIZE {
        return core::ptr::null_mut();
    }

    let old_layout = match Layout::from_size_align(old_size, align) {
        Ok(l) => l,
        Err(_) => return core::ptr::null_mut(),
    };

    #[cfg(not(feature = "std"))]
    {
        alloc::alloc::realloc(ptr, old_layout, new_size)
    }
    
    #[cfg(feature = "std")]
    {
        std::alloc::realloc(ptr, old_layout, new_size)
    }
}

// ---------------------------------------------------------------------------
// DMA buffer management
// ---------------------------------------------------------------------------

/// Allocate DMA-capable buffer
///
/// Returns a buffer descriptor with both physical and virtual addresses.
/// Maximum allocation size is 16MB to prevent resource exhaustion.
#[no_mangle]
pub extern "C" fn staros_memory_alloc_dma_buffer(size: usize) -> FFIResult<FFIBuffer> {
    if size == 0 {
        return FFIResult::err(FFIError::InvalidParameter);
    }

    // Limit DMA buffer size to 16MB to prevent resource exhaustion
    const MAX_DMA_SIZE: usize = 16 * 1024 * 1024;
    if size > MAX_DMA_SIZE {
        return FFIResult::err(FFIError::InvalidParameter);
    }

    // Check for overflow in page calculation
    let _num_pages = match size.checked_add(4095) {
        Some(v) => v / 4096,
        None => return FFIResult::err(FFIError::InvalidParameter),
    };

    // Allocate physical memory
    #[cfg(not(feature = "std"))]
    let phys_addr = unsafe {
        match crate::boot::KERNEL.get().phys_allocator.alloc_pages((size + 4095) / 4096) {
            Ok(addr) => addr.as_usize(),
            Err(_) => return FFIResult::err(FFIError::OutOfMemory),
        }
    };
    
    #[cfg(feature = "std")]
    let phys_addr = 0;

    // For now, assume identity mapping (phys == virt in kernel space)
    // In a real implementation, this would use proper virtual memory mapping
    let virt_addr = phys_addr;

    let buffer = FFIBuffer {
        phys_addr: phys_addr as u64,
        virt_addr: virt_addr as u64,
        size,
        flags: ffi_buffer_flags::READABLE 
             | ffi_buffer_flags::WRITABLE 
             | ffi_buffer_flags::DMA_CAPABLE,
    };

    FFIResult::ok(buffer)
}

/// Free DMA buffer
///
/// # Safety
/// - `buffer` must have been allocated by `staros_memory_alloc_dma_buffer`
/// - Buffer must not be in use by DMA operations
#[no_mangle]
pub unsafe extern "C" fn staros_memory_free_dma_buffer(buffer: FFIBuffer) -> FFIError {
    if buffer.phys_addr == 0 || buffer.size == 0 {
        return FFIError::InvalidParameter;
    }

    let num_pages = (buffer.size + 4095) / 4096;
    
    #[cfg(not(feature = "std"))]
    {
        let phys_addr = crate::memory::physical::PhysAddr::new(buffer.phys_addr as usize);
        match crate::boot::KERNEL.get().phys_allocator.free_pages(phys_addr, num_pages) {
            Ok(()) => FFIError::Success,
            Err(_) => FFIError::InvalidParameter,
        }
    }
    
    #[cfg(feature = "std")]
    {
        FFIError::Success
    }
}

/// Map physical memory to virtual address space
///
/// # Safety
/// - `phys_addr` must be a valid physical address
/// - Caller must ensure proper cache coherency
#[no_mangle]
pub unsafe extern "C" fn staros_memory_map_physical(
    phys_addr: u64,
    size: usize,
    _flags: u32,
) -> FFIResult<u64> {
    if phys_addr == 0 || size == 0 {
        return FFIResult::err(FFIError::InvalidParameter);
    }

    // In a real implementation, this would create a virtual mapping
    // For now, assume identity mapping in kernel space
    let virt_addr = phys_addr;

    FFIResult::ok(virt_addr)
}

/// Unmap virtual memory
///
/// # Safety
/// - `virt_addr` must have been returned by `staros_memory_map_physical`
#[no_mangle]
pub unsafe extern "C" fn staros_memory_unmap(virt_addr: u64, size: usize) -> FFIError {
    if virt_addr == 0 || size == 0 {
        return FFIError::InvalidParameter;
    }

    // In a real implementation, this would remove the virtual mapping
    FFIError::Success
}

// ---------------------------------------------------------------------------
// Cache operations
// ---------------------------------------------------------------------------

/// Flush cache for a memory region (for DMA)
///
/// # Safety
/// - `addr` must point to valid memory of at least `size` bytes
#[no_mangle]
pub unsafe extern "C" fn staros_memory_cache_flush(addr: u64, size: usize) -> FFIError {
    if addr == 0 || size == 0 {
        return FFIError::InvalidParameter;
    }

    // ARM64 cache flush
    #[cfg(target_arch = "aarch64")]
    {
        let start = addr as usize;
        let end = start + size;
        let cache_line_size = 64; // Typical ARM64 cache line size

        let mut current = start & !(cache_line_size - 1);
        while current < end {
            core::arch::asm!(
                "dc cvac, {0}",
                in(reg) current,
                options(nostack)
            );
            current += cache_line_size;
        }

        // Data synchronization barrier
        core::arch::asm!("dsb sy", options(nostack));
    }

    FFIError::Success
}

/// Invalidate cache for a memory region (before DMA read)
///
/// # Safety
/// - `addr` must point to valid memory of at least `size` bytes
#[no_mangle]
pub unsafe extern "C" fn staros_memory_cache_invalidate(addr: u64, size: usize) -> FFIError {
    if addr == 0 || size == 0 {
        return FFIError::InvalidParameter;
    }

    // ARM64 cache invalidate
    #[cfg(target_arch = "aarch64")]
    {
        let start = addr as usize;
        let end = start + size;
        let cache_line_size = 64;

        let mut current = start & !(cache_line_size - 1);
        while current < end {
            core::arch::asm!(
                "dc ivac, {0}",
                in(reg) current,
                options(nostack)
            );
            current += cache_line_size;
        }

        core::arch::asm!("dsb sy", options(nostack));
    }

    FFIError::Success
}

// ---------------------------------------------------------------------------
// Memory statistics
// ---------------------------------------------------------------------------

/// Get memory allocation statistics
#[no_mangle]
pub extern "C" fn staros_memory_get_stats() -> FFIResult<FFIMemoryInfo> {
    // Memory stats would require access to heap allocator internals
    // Return basic info for now
    let info = FFIMemoryInfo {
        total_bytes: 0,
        used_bytes: 0,
        free_bytes: 0,
        largest_free_block: 0,
    };

    FFIResult::ok(info)
}

/// Get total physical memory size
#[no_mangle]
pub extern "C" fn staros_memory_get_total_physical() -> usize {
    0 // Would need global PhysicalAllocator instance
}

/// Get free physical memory size
#[no_mangle]
pub extern "C" fn staros_memory_get_free_physical() -> usize {
    0
}

/// Get used physical memory size
#[no_mangle]
pub extern "C" fn staros_memory_get_used_physical() -> usize {
    0
}

// ---------------------------------------------------------------------------
// Zero-copy buffer sharing
// ---------------------------------------------------------------------------

/// Create a shared buffer for zero-copy data transfer
///
/// Allocates a buffer that can be shared between kernel and userspace
/// without copying data.
#[no_mangle]
pub extern "C" fn staros_memory_create_shared_buffer(size: usize) -> FFIResult<FFIBuffer> {
    staros_memory_alloc_dma_buffer(size)
}

/// Destroy a shared buffer
///
/// # Safety
/// - `buffer` must have been created by `staros_memory_create_shared_buffer`
/// - No references to the buffer must exist
#[no_mangle]
pub unsafe extern "C" fn staros_memory_destroy_shared_buffer(buffer: FFIBuffer) -> FFIError {
    staros_memory_free_dma_buffer(buffer)
}

/// Get buffer virtual address for CPU access
#[no_mangle]
pub extern "C" fn staros_memory_get_buffer_virt_addr(buffer: FFIBuffer) -> u64 {
    buffer.virt_addr
}

/// Get buffer physical address for DMA access
#[no_mangle]
pub extern "C" fn staros_memory_get_buffer_phys_addr(buffer: FFIBuffer) -> u64 {
    buffer.phys_addr
}

/// Get buffer size
#[no_mangle]
pub extern "C" fn staros_memory_get_buffer_size(buffer: FFIBuffer) -> usize {
    buffer.size
}

/// Check if buffer is DMA-capable
#[no_mangle]
pub extern "C" fn staros_memory_is_buffer_dma_capable(buffer: FFIBuffer) -> bool {
    (buffer.flags & ffi_buffer_flags::DMA_CAPABLE) != 0
}

// ---------------------------------------------------------------------------
// Memory copy operations
// ---------------------------------------------------------------------------

/// Copy memory (safe wrapper around memcpy)
///
/// # Safety
/// - `dst` and `src` must point to valid memory of at least `size` bytes
/// - Regions must not overlap (use `staros_memory_move` for overlapping regions)
#[no_mangle]
pub unsafe extern "C" fn staros_memory_copy(
    dst: *mut u8,
    src: *const u8,
    size: usize,
) -> FFIError {
    if dst.is_null() || src.is_null() || size == 0 {
        return FFIError::InvalidParameter;
    }

    // Bounds check: prevent wrapping past address space
    if (dst as usize).checked_add(size).is_none() 
        || (src as usize).checked_add(size).is_none() {
        return FFIError::InvalidParameter;
    }

    // Check for overlap (undefined behavior with copy_nonoverlapping)
    let dst_range = dst as usize..(dst as usize + size);
    let src_start = src as usize;
    if dst_range.contains(&src_start) || dst_range.contains(&(src_start + size - 1)) {
        return FFIError::InvalidParameter;
    }

    core::ptr::copy_nonoverlapping(src, dst, size);
    FFIError::Success
}

/// Move memory (handles overlapping regions)
///
/// # Safety
/// - `dst` and `src` must point to valid memory of at least `size` bytes
#[no_mangle]
pub unsafe extern "C" fn staros_memory_move(
    dst: *mut u8,
    src: *const u8,
    size: usize,
) -> FFIError {
    if dst.is_null() || src.is_null() || size == 0 {
        return FFIError::InvalidParameter;
    }

    // Bounds check: prevent wrapping past address space
    if (dst as usize).checked_add(size).is_none() 
        || (src as usize).checked_add(size).is_none() {
        return FFIError::InvalidParameter;
    }

    core::ptr::copy(src, dst, size);
    FFIError::Success
}

/// Set memory to a value
///
/// # Safety
/// - `ptr` must point to valid memory of at least `size` bytes
#[no_mangle]
pub unsafe extern "C" fn staros_memory_set(
    ptr: *mut u8,
    value: u8,
    size: usize,
) -> FFIError {
    if ptr.is_null() || size == 0 {
        return FFIError::InvalidParameter;
    }

    // Bounds check: prevent wrapping past address space
    if (ptr as usize).checked_add(size).is_none() {
        return FFIError::InvalidParameter;
    }

    core::ptr::write_bytes(ptr, value, size);
    FFIError::Success
}

/// Compare memory regions
///
/// Returns 0 if equal, <0 if a < b, >0 if a > b
///
/// # Safety
/// - `a` and `b` must point to valid memory of at least `size` bytes
#[no_mangle]
pub unsafe extern "C" fn staros_memory_compare(
    a: *const u8,
    b: *const u8,
    size: usize,
) -> i32 {
    if a.is_null() || b.is_null() || size == 0 {
        return 0;
    }

    // Bounds check: prevent wrapping past address space
    if (a as usize).checked_add(size).is_none() 
        || (b as usize).checked_add(size).is_none() {
        return 0;
    }

    let slice_a = core::slice::from_raw_parts(a, size);
    let slice_b = core::slice::from_raw_parts(b, size);

    for i in 0..size {
        if slice_a[i] < slice_b[i] {
            return -1;
        } else if slice_a[i] > slice_b[i] {
            return 1;
        }
    }

    0
}

// ---------------------------------------------------------------------------
// Page allocation (for advanced use)
// ---------------------------------------------------------------------------

/// Allocate physical pages
///
/// Returns physical address of the first page, or 0 on failure.
#[no_mangle]
pub extern "C" fn staros_memory_alloc_pages(_num_pages: usize) -> u64 {
    // Would need global PhysicalAllocator instance
    0
}

/// Free physical pages
///
/// # Safety
/// - `phys_addr` must have been returned by `staros_memory_alloc_pages`
/// - `num_pages` must match the original allocation
#[no_mangle]
pub unsafe extern "C" fn staros_memory_free_pages(_phys_addr: u64, _num_pages: usize) {
    // Would need global PhysicalAllocator instance
}

/// Get page size
#[no_mangle]
pub extern "C" fn staros_memory_get_page_size() -> usize {
    4096 // Standard ARM64 page size
}
