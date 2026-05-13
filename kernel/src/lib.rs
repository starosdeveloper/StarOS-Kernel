#![cfg_attr(not(any(test, feature = "std")), no_std)]

//! STAR OS Microkernel - Production Ready
//!
//! Phase 1: Memory Management

#[cfg(not(any(test, feature = "std")))]
#[macro_use]
extern crate alloc;

#[cfg(any(test, feature = "std"))]
#[macro_use]
extern crate std;

// Make `alloc` available as a crate alias in std/test builds
// so that `use alloc::vec::Vec` etc. work in both modes.
#[cfg(any(test, feature = "std"))]
extern crate alloc;


// Core modules
pub mod prelude;
pub mod error;
pub mod klog;
pub mod sync;
pub mod logger;
pub mod memory;
pub mod process;
pub mod interrupts;
pub mod syscall;
pub mod capabilities;
pub mod async_ipc;
pub mod power;
pub mod stack_guard;
pub mod drivers;
#[cfg(not(test))]
pub mod display_server;
#[cfg(not(test))]
pub mod input;
#[cfg(not(test))]
pub mod safety;
#[cfg(not(test))]
pub mod devicetree;
#[cfg(not(test))]
pub mod soc;
#[cfg(not(test))]
pub mod boot_integration;
#[cfg(not(test))]
pub mod real_device;
#[cfg(not(test))]
pub mod debug_tools;
#[cfg(not(test))]
pub mod crypto;
pub mod net;
pub mod fs;
pub mod elf;
pub mod signals;
pub mod time;
pub mod sleep;

// FFI layer for Kotlin/Native UI integration
#[cfg(not(test))]
pub mod ffi;

#[cfg(not(feature = "std"))]
pub mod boot;

/// Kernel heap allocator - bump allocator with 1MB static heap
/// Provides actual allocation for no_std kernel builds.
/// Once physical memory is mapped, call activate_heap_allocator() to switch
/// to the buddy+slab allocator from memory/heap.rs.
#[cfg(not(feature = "std"))]
mod kernel_alloc {
    use core::alloc::{GlobalAlloc, Layout};
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use crate::memory::heap::HeapAllocator;

    /// 1MB static kernel heap for early boot allocations
    const HEAP_SIZE: usize = 1024 * 1024;

    #[repr(C, align(4096))]
    struct HeapStorage([u8; HEAP_SIZE]);

    static mut HEAP: HeapStorage = HeapStorage([0; HEAP_SIZE]);
    static HEAP_POS: AtomicUsize = AtomicUsize::new(0);
    static HEAP_READY: AtomicBool = AtomicBool::new(false);

    pub struct KernelAllocator;

    /// Activate the buddy+slab heap allocator, replacing the bump allocator.
    pub fn activate_heap_allocator() {
        HEAP_READY.store(true, Ordering::Release);
    }

    unsafe impl GlobalAlloc for KernelAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if HEAP_READY.load(Ordering::Acquire) {
                return HeapAllocator::global_alloc(layout);
            }

            let size = layout.size();
            let align = layout.align();

            loop {
                let pos = HEAP_POS.load(Ordering::Relaxed);
                let aligned = (pos + align - 1) & !(align - 1);
                let new_pos = aligned + size;

                if new_pos > HEAP_SIZE {
                    return core::ptr::null_mut();
                }

                if HEAP_POS.compare_exchange_weak(
                    pos, new_pos, Ordering::AcqRel, Ordering::Relaxed
                ).is_ok() {
                    return HEAP.0.as_mut_ptr().add(aligned);
                }
            }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if HEAP_READY.load(Ordering::Acquire) {
                return HeapAllocator::global_dealloc(ptr, layout);
            }
            // Bump allocator doesn't free individual allocations.
        }
    }

    unsafe impl Send for KernelAllocator {}
    unsafe impl Sync for KernelAllocator {}
}

/// Activate the full heap allocator (buddy+slab), replacing the early bump allocator.
#[cfg(not(feature = "std"))]
pub fn activate_heap_allocator() {
    kernel_alloc::activate_heap_allocator();
}

#[cfg(not(feature = "std"))]
#[global_allocator]
static ALLOCATOR: kernel_alloc::KernelAllocator = kernel_alloc::KernelAllocator;

// Panic handler moved to boot module



