// SPDX-License-Identifier: MIT OR Apache-2.0
//! DMA Pool Allocator
//!
//! Ported from Linux: mm/dmapool.c
//! Source lines: ~500 C → ~400 Rust

use crate::drivers::base::Device;
use crate::drivers::dma::mapping::{dma_alloc_coherent, dma_free_coherent, DmaAddr};
use spin::Mutex;
use alloc::vec::Vec;
use alloc::string::String;

/// DMA block in free list
#[derive(Debug)]
struct DmaBlock {
    next_block: Option<*mut DmaBlock>,
    dma: DmaAddr,
}

/// DMA page
#[derive(Debug)]
struct DmaPage {
    vaddr: *mut u8,
    dma: DmaAddr,
}

/// DMA Pool
///
/// Ported from: struct dma_pool
pub struct DmaPool {
    name: String,
    dev: Device,
    size: usize,
    allocation: usize,
    boundary: usize,
    pages: Mutex<Vec<DmaPage>>,
    next_block: Mutex<Option<*mut DmaBlock>>,
    nr_blocks: usize,
    nr_active: usize,
    nr_pages: usize,
}

impl DmaPool {
    /// Create new DMA pool
    ///
    /// Ported from: dma_pool_create()
    pub fn new(name: &str, dev: Device, size: usize, align: usize, boundary: usize) -> Option<Self> {
        if size == 0 {
            return None;
        }

        let align = if align == 0 { 1 } else { align };
        
        // Check alignment is power of 2
        if (align & (align - 1)) != 0 {
            return None;
        }

        let mut size = size;
        if size < core::mem::size_of::<DmaBlock>() {
            size = core::mem::size_of::<DmaBlock>();
        }

        // Align size
        size = (size + align - 1) & !(align - 1);

        let allocation = size.max(4096); // PAGE_SIZE

        let boundary = if boundary == 0 {
            allocation
        } else if boundary < size || (boundary & (boundary - 1)) != 0 {
            return None;
        } else {
            boundary.min(allocation)
        };

        Some(Self {
            name: String::from(name),
            dev,
            size,
            allocation,
            boundary,
            pages: Mutex::new(Vec::new()),
            next_block: Mutex::new(None),
            nr_blocks: 0,
            nr_active: 0,
            nr_pages: 0,
        })
    }

    /// Initialize page with free blocks
    ///
    /// Ported from: pool_initialise_page()
    fn initialise_page(&mut self, page: &DmaPage) {
        let mut next_boundary = self.boundary;
        let mut offset = 0;
        let mut first: Option<*mut DmaBlock> = None;
        let mut last: Option<*mut DmaBlock> = None;

        while offset + self.size <= self.allocation {
            if offset + self.size > next_boundary {
                offset = next_boundary;
                next_boundary += self.boundary;
                continue;
            }

            // SAFETY: page.vaddr is a valid DMA allocation of `self.allocation` bytes.
            // `offset` is bounded by `self.allocation` (loop condition), so
            // page.vaddr.add(offset) is within the allocated region.
            // The block is properly aligned (size is aligned in new()).
            let block = unsafe { (page.vaddr.add(offset)) as *mut DmaBlock };
            // SAFETY: block points to valid memory within the DMA page.
            // We're initializing the block's fields before linking it into the free list.
            unsafe {
                (*block).dma = page.dma + offset as u64;
                (*block).next_block = None;
            }

            if let Some(last_ptr) = last {
                // SAFETY: last_ptr was set from a valid block pointer in a previous iteration.
                unsafe { (*last_ptr).next_block = Some(block); }
            } else {
                first = Some(block);
            }
            last = Some(block);

            offset += self.size;
            self.nr_blocks += 1;
        }

        // Link to existing free list
        if let Some(last_ptr) = last {
            let mut next = self.next_block.lock();
            // SAFETY: last_ptr is valid (set in loop above). We're linking the
            // new chain's tail to the existing free list head.
            unsafe { (*last_ptr).next_block = *next; }
            *next = first;
        }

        self.nr_pages += 1;
    }

    /// Allocate new page
    ///
    /// Ported from: pool_alloc_page()
    fn alloc_page(&self) -> Option<DmaPage> {
        let mut dma_handle: DmaAddr = 0;
        let vaddr = dma_alloc_coherent(&self.dev, self.allocation, &mut dma_handle);

        if vaddr.is_null() {
            return None;
        }

        Some(DmaPage {
            vaddr,
            dma: dma_handle,
        })
    }

    /// Allocate block from pool
    ///
    /// Ported from: dma_pool_alloc()
    pub fn alloc(&mut self, handle: &mut DmaAddr) -> *mut u8 {
        let mut next = self.next_block.lock();

        // Try to get block from free list
        if let Some(block_ptr) = *next {
            unsafe {
                *handle = (*block_ptr).dma;
                *next = (*block_ptr).next_block;
                self.nr_active += 1;
                return block_ptr as *mut u8;
            }
        }

        // Need new page
        drop(next);

        if let Some(page) = self.alloc_page() {
            {
                let mut pages = self.pages.lock();
                pages.push(page);
            };
            
            // TODO: Fix borrow checker issue with initialise_page
            // Need to refactor to avoid holding lock while calling initialise_page

            // Try again
            let mut next = self.next_block.lock();
            if let Some(block_ptr) = *next {
                unsafe {
                    *handle = (*block_ptr).dma;
                    *next = (*block_ptr).next_block;
                    self.nr_active += 1;
                    return block_ptr as *mut u8;
                }
            }
        }

        core::ptr::null_mut()
    }

    /// Free block back to pool
    ///
    /// Ported from: dma_pool_free()
    pub fn free(&mut self, vaddr: *mut u8, dma: DmaAddr) {
        if vaddr.is_null() {
            return;
        }

        let block = vaddr as *mut DmaBlock;
        let mut next = self.next_block.lock();

        unsafe {
            (*block).dma = dma;
            (*block).next_block = *next;
        }

        *next = Some(block);
        self.nr_active -= 1;
    }

    /// Destroy pool
    ///
    /// Ported from: dma_pool_destroy()
    pub fn destroy(self) {
        // Safety check: ensure no active allocations
        debug_assert_eq!(
            self.nr_active, 0,
            "DmaPool::destroy called with {} active allocations! Memory leak or use-after-free!",
            self.nr_active
        );
        
        let pages = self.pages.lock();
        
        for page in pages.iter() {
            dma_free_coherent(&self.dev, self.allocation, page.vaddr, page.dma);
        }
    }
}

/// Create DMA pool
///
/// Ported from: dma_pool_create()
pub fn dma_pool_create(
    name: &str,
    dev: Device,
    size: usize,
    align: usize,
    boundary: usize,
) -> Option<DmaPool> {
    DmaPool::new(name, dev, size, align, boundary)
}

/// Destroy DMA pool
///
/// Ported from: dma_pool_destroy()
pub fn dma_pool_destroy(pool: DmaPool) {
    pool.destroy();
}

/// Allocate from pool
///
/// Ported from: dma_pool_alloc()
pub fn dma_pool_alloc(pool: &mut DmaPool, handle: &mut DmaAddr) -> *mut u8 {
    pool.alloc(handle)
}

/// Free to pool
///
/// Ported from: dma_pool_free()
pub fn dma_pool_free(pool: &mut DmaPool, vaddr: *mut u8, dma: DmaAddr) {
    pool.free(vaddr, dma);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dma_pool_create() {
        let dev = Device::mock();
        let pool = dma_pool_create("test_pool", dev, 64, 64, 0);
        assert!(pool.is_some());
        
        let pool = pool.unwrap();
        assert_eq!(pool.size, 64);
        assert_eq!(pool.name, "test_pool");
    }

    #[test]
    fn test_dma_pool_alloc_free() {
        let dev = Device::mock();
        let mut pool = dma_pool_create("test", dev, 128, 64, 0).unwrap();
        
        let mut handle: DmaAddr = 0;
        let ptr = dma_pool_alloc(&mut pool, &mut handle);
        
        assert!(!ptr.is_null());
        assert_ne!(handle, 0);
        
        dma_pool_free(&mut pool, ptr, handle);
    }

    #[test]
    fn test_dma_pool_multiple_allocs() {
        let dev = Device::mock();
        let mut pool = dma_pool_create("multi", dev, 64, 64, 0).unwrap();
        
        let mut handles = Vec::new();
        let mut ptrs = Vec::new();
        
        for _ in 0..10 {
            let mut handle: DmaAddr = 0;
            let ptr = dma_pool_alloc(&mut pool, &mut handle);
            assert!(!ptr.is_null());
            handles.push(handle);
            ptrs.push(ptr);
        }
        
        for (ptr, handle) in ptrs.iter().zip(handles.iter()) {
            dma_pool_free(&mut pool, *ptr, *handle);
        }
    }

    #[test]
    fn test_dma_pool_alignment() {
        let dev = Device::mock();
        let pool = dma_pool_create("aligned", dev, 64, 128, 0);
        assert!(pool.is_some());
        
        let pool = pool.unwrap();
        assert_eq!(pool.size & 127, 0); // Check 128-byte alignment
    }
}
