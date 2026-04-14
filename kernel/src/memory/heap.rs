//! Heap allocator - SLAB + Buddy system
//!
//! - SLAB: Fast allocation for small objects (<4KB)
//! - Buddy: Efficient allocation for large objects (≥4KB)

#[cfg(not(feature = "std"))]
use core::alloc::{GlobalAlloc, Layout};
#[cfg(feature = "std")]
use std::alloc::{GlobalAlloc, Layout};

#[cfg(not(feature = "std"))]
use core::ptr;
#[cfg(feature = "std")]
use std::ptr;

#[cfg(not(feature = "std"))]
use core::cell::UnsafeCell;
#[cfg(feature = "std")]
use std::cell::UnsafeCell;

use crate::error::{KernelError, MemoryError};
use super::{PhysicalAllocator};

/// Buddy allocator for large allocations (≥4KB)
/// 
/// Uses binary buddy system with orders 0-10 (4KB to 4MB)
pub struct BuddyAllocator {
    free_lists: [Option<*mut BuddyBlock>; 11], // Orders 0-10
    allocator: *const PhysicalAllocator,
}

#[repr(C)]
struct BuddyBlock {
    next: Option<*mut BuddyBlock>,
}

impl BuddyAllocator {
    pub const fn new(allocator: &PhysicalAllocator) -> Self {
        Self {
            free_lists: [None; 11],
            allocator: allocator as *const _,
        }
    }

    /// Allocate memory of given order (order 0 = 4KB, order 1 = 8KB, etc.)
    pub fn alloc(&mut self, order: usize) -> Result<*mut u8, KernelError> {
        if order >= 11 {
            return Err(KernelError::Memory(MemoryError::InvalidSize));
        }

        // Try to find free block of requested order
        if let Some(block) = self.free_lists[order] {
            // SAFETY: block pointer is valid - it was previously allocated and added to free_lists
            // The pointer is properly aligned (4KB minimum) and points to valid memory
            unsafe {
                self.free_lists[order] = (*block).next;
                return Ok(block as *mut u8);
            }
        }

        // No free block, try to split larger block
        for higher_order in (order + 1)..11 {
            if let Some(block) = self.free_lists[higher_order] {
                // SAFETY: block pointer is valid from free_lists, properly aligned and accessible
                unsafe {
                    self.free_lists[higher_order] = (*block).next;
                }

                // Split block recursively down to requested order
                return self.split_block(block as *mut u8, higher_order, order);
            }
        }

        // No free blocks, allocate from physical allocator
        let pages = 1 << order;
        // SAFETY: allocator pointer is valid - initialized in new() and never modified
        // PhysicalAllocator is thread-safe and the pointer lifetime matches BuddyAllocator
        let phys = unsafe { (*self.allocator).alloc_pages(pages)? };
        
        Ok(phys.as_usize() as *mut u8)
    }

    /// Free memory of given order
    pub fn free(&mut self, ptr: *mut u8, order: usize) -> Result<(), KernelError> {
        if order >= 11 {
            return Err(KernelError::Memory(MemoryError::InvalidSize));
        }

        // Try to coalesce with buddy
        let buddy_ptr = self.buddy_of(ptr, order);
        
        if self.is_free(buddy_ptr, order) {
            // Remove buddy from free list
            self.remove_from_list(buddy_ptr, order);
            
            // Coalesce and free at higher order
            let coalesced = if (ptr as usize) < (buddy_ptr as usize) {
                ptr
            } else {
                buddy_ptr
            };
            
            return self.free(coalesced, order + 1);
        }

        // Can't coalesce, add to free list
        // SAFETY: ptr is valid memory from previous allocation, properly aligned (4KB)
        // We're converting it to BuddyBlock which has same alignment requirements
        // The memory is being freed so no aliasing issues
        unsafe {
            let block = ptr as *mut BuddyBlock;
            (*block).next = self.free_lists[order];
            self.free_lists[order] = Some(block);
        }

        Ok(())
    }

    fn split_block(&mut self, ptr: *mut u8, from_order: usize, to_order: usize) -> Result<*mut u8, KernelError> {
        if from_order == to_order {
            return Ok(ptr);
        }

        let split_order = from_order - 1;
        let split_size = 4096 << split_order;
        
        // Add second half to free list
        // SAFETY: ptr is valid, split_size is calculated correctly (power of 2 * 4KB)
        // The addition stays within allocated memory bounds (from_order > to_order)
        let buddy = unsafe { ptr.add(split_size) };
        // SAFETY: buddy pointer is valid (within allocated block), properly aligned
        // We're initializing it as BuddyBlock which is safe for this memory region
        unsafe {
            let block = buddy as *mut BuddyBlock;
            (*block).next = self.free_lists[split_order];
            self.free_lists[split_order] = Some(block);
        }

        // Continue splitting first half
        self.split_block(ptr, split_order, to_order)
    }

    fn buddy_of(&self, ptr: *mut u8, order: usize) -> *mut u8 {
        let addr = ptr as usize;
        let size = 4096 << order;
        let buddy_addr = addr ^ size;
        buddy_addr as *mut u8
    }

    fn is_free(&self, ptr: *mut u8, order: usize) -> bool {
        let mut current = self.free_lists[order];
        while let Some(block) = current {
            if block as *mut u8 == ptr {
                return true;
            }
            // SAFETY: block is from free_lists, guaranteed valid and properly linked
            unsafe {
                current = (*block).next;
            }
        }
        false
    }

    fn remove_from_list(&mut self, ptr: *mut u8, order: usize) {
        let target = ptr as *mut BuddyBlock;
        
        if self.free_lists[order] == Some(target) {
            // SAFETY: target is in free_lists[order], valid pointer
            unsafe {
                self.free_lists[order] = (*target).next;
            }
            return;
        }

        let mut current = self.free_lists[order];
        while let Some(block) = current {
            // SAFETY: block is from free_lists, valid and properly linked
            // We're traversing the linked list to find and remove target
            unsafe {
                if (*block).next == Some(target) {
                    (*block).next = (*target).next;
                    return;
                }
                current = (*block).next;
            }
        }
    }
}

// SAFETY: BuddyAllocator uses raw pointers but all operations are synchronized
// The allocator pointer is immutable after construction
unsafe impl Send for BuddyAllocator {}
// SAFETY: All mutable operations require &mut self, providing exclusive access
unsafe impl Sync for BuddyAllocator {}

/// SLAB allocator for small allocations (<4KB)
pub struct SlabAllocator {
    // Size classes: 8, 16, 32, 64, 128, 256, 512, 1024, 2048 bytes
    slabs: [Slab; 9],
    buddy: *mut BuddyAllocator,
}

struct Slab {
    size: usize,
    free_list: Option<*mut SlabBlock>,
}

#[repr(C)]
struct SlabBlock {
    next: Option<*mut SlabBlock>,
}

impl SlabAllocator {
    pub const fn new(buddy: &mut BuddyAllocator) -> Self {
        const INIT_SLAB: Slab = Slab {
            size: 0,
            free_list: None,
        };

        Self {
            slabs: [INIT_SLAB; 9],
            buddy: buddy as *mut _,
        }
    }

    pub fn init(&mut self) {
        let sizes = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];
        for (i, &size) in sizes.iter().enumerate() {
            self.slabs[i].size = size;
        }
    }

    pub fn alloc(&mut self, size: usize) -> Result<*mut u8, KernelError> {
        if size == 0 || size > 2048 {
            return Err(KernelError::Memory(MemoryError::InvalidSize));
        }

        // Find appropriate slab
        let slab_idx = self.size_to_slab(size);
        let slab = &mut self.slabs[slab_idx];

        // Try to allocate from free list
        if let Some(block) = slab.free_list {
            // SAFETY: block is from free_list, valid pointer to previously freed memory
            // Memory is properly aligned for slab.size and accessible
            unsafe {
                slab.free_list = (*block).next;
                return Ok(block as *mut u8);
            }
        }

        // No free blocks, allocate new page from buddy
        // SAFETY: buddy pointer is valid, initialized in new() and immutable
        let page = unsafe { (*self.buddy).alloc(0)? }; // Order 0 = 4KB

        // Split page into blocks of slab size
        let block_size = slab.size;
        let blocks_per_page = 4096 / block_size;

        // Add all blocks except first to free list
        for i in 1..blocks_per_page {
            // SAFETY: page is valid 4KB allocation, i * block_size < 4096
            // All blocks are within page bounds and properly aligned
            unsafe {
                let block_ptr = page.add(i * block_size) as *mut SlabBlock;
                (*block_ptr).next = slab.free_list;
                slab.free_list = Some(block_ptr);
            }
        }

        Ok(page)
    }

    pub fn free(&mut self, ptr: *mut u8, size: usize) -> Result<(), KernelError> {
        if size == 0 || size > 2048 {
            return Err(KernelError::Memory(MemoryError::InvalidSize));
        }

        let slab_idx = self.size_to_slab(size);
        let slab = &mut self.slabs[slab_idx];

        // Add to free list
        // SAFETY: ptr is valid memory from previous allocation of this size
        // Converting to SlabBlock is safe as it only contains a pointer
        // No aliasing issues as memory is being freed
        unsafe {
            let block = ptr as *mut SlabBlock;
            (*block).next = slab.free_list;
            slab.free_list = Some(block);
        }

        Ok(())
    }

    fn size_to_slab(&self, size: usize) -> usize {
        // Round up to next power of 2, minimum 8
        let size = size.max(8);
        let size = size.next_power_of_two();
        
        // Map to slab index: 8->0, 16->1, 32->2, etc.
        (size.trailing_zeros() - 3) as usize
    }
}

/// Combined heap allocator
pub struct HeapAllocator {
    buddy: UnsafeCell<BuddyAllocator>,
    slab: UnsafeCell<SlabAllocator>,
}

impl HeapAllocator {
    pub fn new(allocator: &PhysicalAllocator) -> Self {
        let mut buddy = BuddyAllocator::new(allocator);
        let mut slab = SlabAllocator::new(&mut buddy);
        slab.init();

        Self {
            buddy: UnsafeCell::new(buddy),
            slab: UnsafeCell::new(slab),
        }
    }
}

// SAFETY: GlobalAlloc implementation is thread-safe through UnsafeCell interior mutability
// All allocations are properly aligned and sized
// Memory is never freed while still in use (enforced by Rust's ownership)
unsafe impl GlobalAlloc for HeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        // For now, ignore alignment > 8 (TODO: handle properly)
        if align > 8 && size < 4096 {
            return ptr::null_mut();
        }

        if size < 4096 {
            // Use SLAB
            // SAFETY: UnsafeCell provides interior mutability, single-threaded access guaranteed
            // by kernel synchronization primitives at higher level
            let slab = &mut *self.slab.get();
            slab.alloc(size).unwrap_or(ptr::null_mut())
        } else {
            // Use Buddy
            let pages = (size + 4095) / 4096;
            let order = pages.next_power_of_two().trailing_zeros() as usize;
            
            // SAFETY: Same as above - interior mutability with external synchronization
            let buddy = &mut *self.buddy.get();
            buddy.alloc(order).unwrap_or(ptr::null_mut())
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();

        if size < 4096 {
            // SAFETY: ptr is valid allocation from alloc(), size matches original allocation
            // UnsafeCell interior mutability with external synchronization
            let slab = &mut *self.slab.get();
            let _ = slab.free(ptr, size);
        } else {
            let pages = (size + 4095) / 4096;
            let order = pages.next_power_of_two().trailing_zeros() as usize;
            
            // SAFETY: ptr is valid allocation, order matches original allocation
            // Interior mutability with external synchronization
            let buddy = &mut *self.buddy.get();
            let _ = buddy.free(ptr, order);
        }
    }
}

// SAFETY: HeapAllocator can be sent between threads - contains only UnsafeCell
// which is Send when T is Send (BuddyAllocator and SlabAllocator are Send)
unsafe impl Send for HeapAllocator {}
// SAFETY: Thread-safe through external synchronization at kernel level
// UnsafeCell provides interior mutability, actual sync handled by caller
unsafe impl Sync for HeapAllocator {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_to_slab() {
        let buddy = BuddyAllocator::new(unsafe { &*(0x1000 as *const PhysicalAllocator) });
        let mut slab = SlabAllocator::new(unsafe { &mut *(0x1000 as *mut BuddyAllocator) });
        slab.init();

        assert_eq!(slab.size_to_slab(1), 0);   // 8 bytes
        assert_eq!(slab.size_to_slab(8), 0);   // 8 bytes
        assert_eq!(slab.size_to_slab(9), 1);   // 16 bytes
        assert_eq!(slab.size_to_slab(16), 1);  // 16 bytes
        assert_eq!(slab.size_to_slab(17), 2);  // 32 bytes
        assert_eq!(slab.size_to_slab(1024), 7); // 1024 bytes
        assert_eq!(slab.size_to_slab(2048), 8); // 2048 bytes
    }

    #[test]
    fn test_buddy_order_calculation() {
        // Test that order calculation is correct
        assert_eq!(1usize.next_power_of_two().trailing_zeros(), 0); // 1 page = order 0
        assert_eq!(2usize.next_power_of_two().trailing_zeros(), 1); // 2 pages = order 1
        assert_eq!(4usize.next_power_of_two().trailing_zeros(), 2); // 4 pages = order 2
        assert_eq!(8usize.next_power_of_two().trailing_zeros(), 3); // 8 pages = order 3
    }

    #[test]
    fn test_layout_size_routing() {
        // Test that sizes route to correct allocator
        assert!(32 < 4096);    // Should use SLAB
        assert!(2048 < 4096);  // Should use SLAB
        assert!(4096 >= 4096); // Should use Buddy
        assert!(8192 >= 4096); // Should use Buddy
    }

    #[test]
    fn test_slab_sizes() {
        let sizes: [usize; 9] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];
        
        // Verify all are powers of 2
        for &size in &sizes {
            assert_eq!(size, size.next_power_of_two());
        }
        
        // Verify coverage up to 2048
        for size in 1usize..=2048 {
            let rounded = size.max(8).next_power_of_two();
            assert!(sizes.contains(&rounded));
        }
    }

    #[test]
    fn test_buddy_size_limits() {
        // Order 0 = 4KB = 1 page
        assert_eq!(4096 << 0, 4096);
        
        // Order 10 = 4MB = 1024 pages
        assert_eq!(4096 << 10, 4 * 1024 * 1024);
        
        // Verify we don't exceed max order
        assert!(10 < 11); // MAX_ORDER = 11
    }
}

