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

#[cfg(not(feature = "std"))]
use core::alloc::{GlobalAlloc, Layout};

// Core modules
pub mod prelude;
pub mod error;
pub mod sync;
pub mod logger;
pub mod memory;
pub mod process;
pub mod interrupts;
pub mod syscall;
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
pub mod time;

#[cfg(not(feature = "std"))]
pub mod boot;

// Dummy allocator for now (will be replaced with heap allocator in Phase 1.3)
#[cfg(not(feature = "std"))]
struct DummyAllocator;

#[cfg(not(feature = "std"))]
unsafe impl GlobalAlloc for DummyAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[cfg(not(feature = "std"))]
#[global_allocator]
static ALLOCATOR: DummyAllocator = DummyAllocator;

// Panic handler moved to boot module



