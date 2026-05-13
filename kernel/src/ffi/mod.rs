//! FFI layer for Kotlin/Native UI integration
//!
//! This module exports the STAR OS kernel API with C ABI for use from Kotlin/Native.
//! All functions use `#[no_mangle]` and `extern "C"` for stable ABI.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                  Kotlin/Native UI Application                │
//! │                                                              │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
//! │  │ Display  │  │  Input   │  │   IPC    │  │  Memory  │   │
//! │  │ Wrapper  │  │ Wrapper  │  │ Wrapper  │  │ Wrapper  │   │
//! │  └──────────┘  └──────────┘  └──────────┘  └──────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//!                            │
//!                            │ cinterop (.def file)
//!                            ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    FFI Layer (C ABI)                         │
//! │                                                              │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
//! │  │ display  │  │  input   │  │   ipc    │  │  memory  │   │
//! │  │   .rs    │  │   .rs    │  │   .rs    │  │   .rs    │   │
//! │  └──────────┘  └──────────┘  └──────────┘  └──────────┘   │
//! │  ┌──────────┐  ┌──────────┐                                │
//! │  │  system  │  │  types   │                                │
//! │  │   .rs    │  │   .rs    │                                │
//! │  └──────────┘  └──────────┘                                │
//! └─────────────────────────────────────────────────────────────┘
//!                            │
//!                            │ Internal Rust API
//!                            ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   STAR OS Kernel                             │
//! │                                                              │
//! │  display_server │ input │ process │ memory │ drivers        │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Modules
//!
//! - [`types`] - C-compatible type definitions
//! - [`display`] - Display server and compositor API
//! - [`input`] - Input event handling (mouse, keyboard, touch)
//! - [`ipc`] - Inter-process communication
//! - [`memory`] - Memory management and buffer allocation
//! - [`system`] - System management and power control
//!
//! # Usage from Kotlin/Native
//!
//! 1. Create a `.def` file describing the FFI interface
//! 2. Use Kotlin/Native cinterop to generate bindings
//! 3. Create idiomatic Kotlin wrappers around the C API
//!
//! Example `.def` file:
//!
//! ```text
//! headers = staros_ffi.h
//! package = com.staros.kernel
//! libraryPaths = /path/to/kernel
//! staticLibraries = libstaros_kernel.a
//! ```
//!
//! # Safety
//!
//! All FFI functions are marked `unsafe` where appropriate. Kotlin/Native code
//! must ensure:
//!
//! - Pointers are valid and properly aligned
//! - Lifetimes are respected (no use-after-free)
//! - Thread safety (most kernel APIs are not thread-safe)
//! - Proper error handling (check return codes)
//!
//! # Error Handling
//!
//! Most functions return [`FFIError`] or [`FFIResult<T>`]. Always check for
//! errors before using return values.
//!
//! # Memory Management
//!
//! - Buffers allocated by `staros_memory_alloc` must be freed with `staros_memory_free`
//! - Shared memory must be properly attached/detached
//! - DMA buffers must not be freed while DMA is in progress
//!
//! # Threading
//!
//! The kernel is currently single-threaded. Do not call FFI functions from
//! multiple Kotlin/Native threads concurrently.

pub mod types;
pub mod display;
pub mod input;
pub mod ipc;
pub mod memory;
pub mod system;

// Re-export commonly used types
pub use types::*;

// Version information
pub const FFI_VERSION_MAJOR: u32 = 0;
pub const FFI_VERSION_MINOR: u32 = 1;
pub const FFI_VERSION_PATCH: u32 = 0;

/// Get FFI layer version
#[no_mangle]
pub extern "C" fn staros_ffi_get_version() -> u32 {
    (FFI_VERSION_MAJOR << 16) | (FFI_VERSION_MINOR << 8) | FFI_VERSION_PATCH
}

/// Get FFI layer version string
///
/// Returns pointer to static string (no need to free).
#[no_mangle]
pub extern "C" fn staros_ffi_get_version_string() -> *const u8 {
    concat!(
        env!("CARGO_PKG_VERSION"),
        "-ffi"
    ).as_ptr()
}

/// Get FFI layer version string length
#[no_mangle]
pub extern "C" fn staros_ffi_get_version_string_len() -> usize {
    concat!(
        env!("CARGO_PKG_VERSION"),
        "-ffi"
    ).len()
}

/// Initialize FFI layer
///
/// Must be called before using any other FFI functions.
#[no_mangle]
pub extern "C" fn staros_ffi_init() -> FFIError {
    // Perform any necessary initialization
    FFIError::Success
}

/// Shutdown FFI layer
///
/// Should be called before kernel shutdown.
#[no_mangle]
pub extern "C" fn staros_ffi_shutdown() -> FFIError {
    // Perform cleanup
    FFIError::Success
}
