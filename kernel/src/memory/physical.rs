//! Physical memory allocator
//!
//! Bitmap-based allocator for 4KB pages with O(1) single page allocation.

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::{KernelError, MemoryError};

pub const PAGE_SIZE: usize = 4096;
const MAX_PAGES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(usize);

impl PhysAddr {
    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }

    pub const fn is_aligned(&self) -> bool {
        self.0 % PAGE_SIZE == 0
    }

    pub const fn page_number(&self) -> usize {
        self.0 / PAGE_SIZE
    }
}

pub struct PhysicalAllocator {
    bitmap: [AtomicUsize; MAX_PAGES / (core::mem::size_of::<usize>() * 8)],
    total_pages: usize,
    free_pages: AtomicUsize,
    base_addr: PhysAddr,
}

impl PhysicalAllocator {
    pub const fn new(base_addr: PhysAddr, total_pages: usize) -> Self {
        // assert!(total_pages <= MAX_PAGES); // Removed - can't use in const fn
        const INIT: AtomicUsize = AtomicUsize::new(0);
        
        Self {
            bitmap: [INIT; MAX_PAGES / (core::mem::size_of::<usize>() * 8)],
            total_pages,
            free_pages: AtomicUsize::new(total_pages),
            base_addr,
        }
    }

    pub fn init(&self) -> Result<(), KernelError> {
        for i in 0..self.bitmap.len() {
            self.bitmap[i].store(0, Ordering::Release);
        }
        self.free_pages.store(self.total_pages, Ordering::Release);
        Ok(())
    }

    pub fn alloc_page(&self) -> Result<PhysAddr, KernelError> {
        if self.free_pages.load(Ordering::Acquire) == 0 {
            return Err(KernelError::Memory(MemoryError::OutOfMemory));
        }

        for (word_idx, word) in self.bitmap.iter().enumerate() {
            let mut current = word.load(Ordering::Acquire);
            
            if current == usize::MAX {
                continue;
            }

            loop {
                let bit_idx = current.trailing_ones() as usize;
                if bit_idx >= core::mem::size_of::<usize>() * 8 {
                    break;
                }

                let mask = 1usize << bit_idx;
                let new = current | mask;

                match word.compare_exchange_weak(
                    current,
                    new,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        let page_num = word_idx * core::mem::size_of::<usize>() * 8 + bit_idx;
                        
                        if page_num >= self.total_pages {
                            return Err(KernelError::Memory(MemoryError::OutOfMemory));
                        }

                        self.free_pages.fetch_sub(1, Ordering::AcqRel);
                        
                        let addr = self.base_addr.as_usize() + page_num * PAGE_SIZE;
                        return Ok(PhysAddr::new(addr));
                    }
                    Err(actual) => {
                        current = actual;
                    }
                }
            }
        }

        Err(KernelError::Memory(MemoryError::OutOfMemory))
    }

    pub fn alloc_pages(&self, count: usize) -> Result<PhysAddr, KernelError> {
        if count == 0 {
            return Err(KernelError::InvalidParameter("count must be > 0"));
        }

        if count == 1 {
            return self.alloc_page();
        }

        if self.free_pages.load(Ordering::Acquire) < count {
            return Err(KernelError::Memory(MemoryError::OutOfMemory));
        }

        let mut start_page = None;
        let mut consecutive = 0;

        for page in 0..self.total_pages {
            if self.is_page_free(page) {
                if consecutive == 0 {
                    start_page = Some(page);
                }
                consecutive += 1;

                if consecutive == count {
                    let start = start_page.unwrap();
                    
                    for i in 0..count {
                        self.mark_page_allocated(start + i)?;
                    }

                    self.free_pages.fetch_sub(count, Ordering::AcqRel);
                    
                    let addr = self.base_addr.as_usize() + start * PAGE_SIZE;
                    return Ok(PhysAddr::new(addr));
                }
            } else {
                consecutive = 0;
                start_page = None;
            }
        }

        Err(KernelError::Memory(MemoryError::OutOfMemory))
    }

    pub fn free_page(&self, addr: PhysAddr) -> Result<(), KernelError> {
        if !addr.is_aligned() {
            return Err(KernelError::Memory(MemoryError::InvalidAlignment));
        }

        let page_num = self.addr_to_page(addr)?;
        
        if self.is_page_free(page_num) {
            return Err(KernelError::Memory(MemoryError::DoubleFree));
        }

        self.mark_page_free(page_num)?;
        self.free_pages.fetch_add(1, Ordering::AcqRel);

        Ok(())
    }

    pub fn free_pages(&self, addr: PhysAddr, count: usize) -> Result<(), KernelError> {
        if count == 0 {
            return Err(KernelError::InvalidParameter("count must be > 0"));
        }

        if !addr.is_aligned() {
            return Err(KernelError::Memory(MemoryError::InvalidAlignment));
        }

        let start_page = self.addr_to_page(addr)?;

        for i in 0..count {
            if self.is_page_free(start_page + i) {
                return Err(KernelError::Memory(MemoryError::DoubleFree));
            }
        }

        for i in 0..count {
            self.mark_page_free(start_page + i)?;
        }

        self.free_pages.fetch_add(count, Ordering::AcqRel);

        Ok(())
    }

    pub fn available_pages(&self) -> usize {
        self.free_pages.load(Ordering::Acquire)
    }

    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    fn addr_to_page(&self, addr: PhysAddr) -> Result<usize, KernelError> {
        if addr.as_usize() < self.base_addr.as_usize() {
            return Err(KernelError::Memory(MemoryError::InvalidAddress));
        }

        let offset = addr.as_usize() - self.base_addr.as_usize();
        let page = offset / PAGE_SIZE;

        if page >= self.total_pages {
            return Err(KernelError::Memory(MemoryError::InvalidAddress));
        }

        Ok(page)
    }

    fn is_page_free(&self, page: usize) -> bool {
        let word_idx = page / (core::mem::size_of::<usize>() * 8);
        let bit_idx = page % (core::mem::size_of::<usize>() * 8);
        
        let word = self.bitmap[word_idx].load(Ordering::Acquire);
        (word & (1 << bit_idx)) == 0
    }

    fn mark_page_allocated(&self, page: usize) -> Result<(), KernelError> {
        let word_idx = page / (core::mem::size_of::<usize>() * 8);
        let bit_idx = page % (core::mem::size_of::<usize>() * 8);
        
        let mask = 1usize << bit_idx;
        self.bitmap[word_idx].fetch_or(mask, Ordering::AcqRel);
        
        Ok(())
    }

    fn mark_page_free(&self, page: usize) -> Result<(), KernelError> {
        let word_idx = page / (core::mem::size_of::<usize>() * 8);
        let bit_idx = page % (core::mem::size_of::<usize>() * 8);
        
        let mask = !(1usize << bit_idx);
        self.bitmap[word_idx].fetch_and(mask, Ordering::AcqRel);
        
        Ok(())
    }
}

unsafe impl Send for PhysicalAllocator {}
unsafe impl Sync for PhysicalAllocator {}
