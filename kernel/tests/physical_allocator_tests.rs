//! Integration tests for Physical Memory Allocator
//!
//! These tests run with std on the host system

// TEMPORARY: Wrapper module to work around no_std + derive macro issues
// TODO: Replace with proper feature flags when Rust prelude imports are fixed
mod test_wrapper {
    pub use std::vec::Vec;
    pub use std::sync::Arc;
    pub use std::thread;
    
    // Re-export kernel types with std enabled
    pub use staros_kernel::memory::{PhysicalAllocator, PhysAddr, PAGE_SIZE};
    pub use staros_kernel::error::{KernelError, MemoryError};
}

use test_wrapper::*;

fn create_test_allocator() -> PhysicalAllocator {
    let alloc = PhysicalAllocator::new(PhysAddr::new(0x80000000), 1024);
    alloc.init().unwrap();
    alloc
}

#[test]
fn test_alloc_single_page() {
    let alloc = create_test_allocator();
    let page = alloc.alloc_page().unwrap();
    assert!(page.is_aligned());
    assert_eq!(alloc.available_pages(), 1023);
}

#[test]
fn test_alloc_free_page() {
    let alloc = create_test_allocator();
    let page = alloc.alloc_page().unwrap();
    assert_eq!(alloc.available_pages(), 1023);
    alloc.free_page(page).unwrap();
    assert_eq!(alloc.available_pages(), 1024);
}

#[test]
fn test_alloc_multiple_pages() {
    let alloc = create_test_allocator();
    let pages = alloc.alloc_pages(10).unwrap();
    assert!(pages.is_aligned());
    assert_eq!(alloc.available_pages(), 1014);
}

#[test]
fn test_free_multiple_pages() {
    let alloc = create_test_allocator();
    let pages = alloc.alloc_pages(10).unwrap();
    alloc.free_pages(pages, 10).unwrap();
    assert_eq!(alloc.available_pages(), 1024);
}

#[test]
fn test_out_of_memory() {
    let alloc = create_test_allocator();
    for _ in 0..1024 {
        alloc.alloc_page().unwrap();
    }
    assert!(matches!(
        alloc.alloc_page(),
        Err(KernelError::Memory(MemoryError::OutOfMemory))
    ));
}

#[test]
fn test_double_free() {
    let alloc = create_test_allocator();
    let page = alloc.alloc_page().unwrap();
    alloc.free_page(page).unwrap();
    assert!(matches!(
        alloc.free_page(page),
        Err(KernelError::Memory(MemoryError::DoubleFree))
    ));
}

#[test]
fn test_invalid_address() {
    let alloc = create_test_allocator();
    // Use aligned address outside allocator range
    let invalid = PhysAddr::new(0xDEAD0000); // Aligned to 4KB
    let result = alloc.free_page(invalid);
    assert!(matches!(
        result,
        Err(KernelError::Memory(MemoryError::InvalidAddress))
    ));
}

#[test]
fn test_unaligned_address() {
    let alloc = create_test_allocator();
    let unaligned = PhysAddr::new(0x80000001);
    assert!(matches!(
        alloc.free_page(unaligned),
        Err(KernelError::Memory(MemoryError::InvalidAlignment))
    ));
}

#[test]
fn test_zero_pages() {
    let alloc = create_test_allocator();
    assert!(matches!(
        alloc.alloc_pages(0),
        Err(KernelError::InvalidParameter(_))
    ));
}

#[test]
fn test_alloc_free_roundtrip() {
    let alloc = create_test_allocator();
    let mut pages = Vec::new();
    
    // Allocate 100 pages
    for _ in 0..100 {
        pages.push(alloc.alloc_page().unwrap());
    }
    assert_eq!(alloc.available_pages(), 924);
    
    // Free all
    for page in pages {
        alloc.free_page(page).unwrap();
    }
    assert_eq!(alloc.available_pages(), 1024);
}

#[test]
fn test_fragmentation() {
    let alloc = create_test_allocator();
    
    // Allocate every other page
    let mut pages = Vec::new();
    for _ in 0..512 {
        pages.push(alloc.alloc_page().unwrap());
        let _ = alloc.alloc_page(); // This one we keep allocated
    }
    
    // Free the saved pages
    for page in pages {
        alloc.free_page(page).unwrap();
    }
    
    // Should still have 512 free pages, but fragmented
    assert_eq!(alloc.available_pages(), 512);
    
    // Try to allocate 100 contiguous pages - should fail due to fragmentation
    assert!(alloc.alloc_pages(100).is_err());
}

#[test]
fn test_large_allocation() {
    let alloc = create_test_allocator();
    
    // Allocate 500 contiguous pages
    let pages = alloc.alloc_pages(500).unwrap();
    assert_eq!(alloc.available_pages(), 524);
    
    alloc.free_pages(pages, 500).unwrap();
    assert_eq!(alloc.available_pages(), 1024);
}

#[test]
fn test_concurrent_alloc_free() {
    let alloc = Arc::new(create_test_allocator());
    let mut handles = vec![];
    
    // Spawn 4 threads
    for _ in 0..4 {
        let alloc_clone = Arc::clone(&alloc);
        let handle: thread::JoinHandle<()> = thread::spawn(move || {
            for _ in 0..50 {
                if let Ok(page) = alloc_clone.alloc_page() {
                    let _ = alloc_clone.free_page(page);
                }
            }
        });
        handles.push(handle);
    }
    
    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
    
    // All pages should be free
    assert_eq!(alloc.available_pages(), 1024);
}

#[test]
fn test_boundary_conditions() {
    let alloc = create_test_allocator();
    
    // Allocate exactly all pages
    let pages = alloc.alloc_pages(1024).unwrap();
    assert_eq!(alloc.available_pages(), 0);
    
    // Try to allocate one more - should fail
    assert!(alloc.alloc_page().is_err());
    
    // Free all
    alloc.free_pages(pages, 1024).unwrap();
    assert_eq!(alloc.available_pages(), 1024);
}

#[test]
fn test_page_alignment() {
    let alloc = create_test_allocator();
    
    for _ in 0..100 {
        let page = alloc.alloc_page().unwrap();
        assert_eq!(page.as_usize() % PAGE_SIZE, 0);
        alloc.free_page(page).unwrap();
    }
}
