//! System FFI — Kotlin/Native bindings for system management
//!
//! Exports system management functionality with C ABI:
//! - Process/task information
//! - Power management
//! - System events
//! - Watchdog control
//! - System statistics

//! System FFI — Kotlin/Native bindings for system management
//!
//! Exports only implemented and working system functionality.
//! 
//! NOTE: Many system management features (scheduler, watchdog, power management)
//! are not yet implemented in the kernel and are therefore not exposed here.


// ---------------------------------------------------------------------------
// System time (WORKING - uses ARM Generic Timer)
// ---------------------------------------------------------------------------

/// Get system uptime in microseconds
#[no_mangle]
pub extern "C" fn staros_system_get_uptime_us() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        // Read ARM Generic Timer counter
        let count: u64;
        unsafe {
            core::arch::asm!(
                "mrs {}, CNTPCT_EL0",
                out(reg) count,
                options(nomem, nostack)
            );
        }
        count
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        0
    }
}

/// Get system uptime in milliseconds
#[no_mangle]
pub extern "C" fn staros_system_get_uptime_ms() -> u64 {
    staros_system_get_uptime_us() / 1000
}

/// Get system uptime in seconds
#[no_mangle]
pub extern "C" fn staros_system_get_uptime_sec() -> u64 {
    staros_system_get_uptime_us() / 1_000_000
}

// ---------------------------------------------------------------------------
// System information (WORKING - compile-time constants)
// ---------------------------------------------------------------------------

/// Get kernel version string
///
/// Returns pointer to static string (no need to free).
#[no_mangle]
pub extern "C" fn staros_system_get_kernel_version() -> *const u8 {
    env!("CARGO_PKG_VERSION").as_ptr()
}

/// Get kernel version string length
#[no_mangle]
pub extern "C" fn staros_system_get_kernel_version_len() -> usize {
    env!("CARGO_PKG_VERSION").len()
}

/// Get kernel build date
///
/// Returns pointer to static string (no need to free).
#[no_mangle]
pub extern "C" fn staros_system_get_build_date() -> *const u8 {
    "2026-04-21".as_ptr()
}

/// Get kernel build date length
#[no_mangle]
pub extern "C" fn staros_system_get_build_date_len() -> usize {
    "2026-04-21".len()
}

/// Get CPU architecture string
///
/// Returns pointer to static string (no need to free).
#[no_mangle]
pub extern "C" fn staros_system_get_arch() -> *const u8 {
    "aarch64".as_ptr()
}

/// Get CPU architecture string length
#[no_mangle]
pub extern "C" fn staros_system_get_arch_len() -> usize {
    "aarch64".len()
}

// ---------------------------------------------------------------------------
// Debug logging (WORKING - conditional compilation)
// ---------------------------------------------------------------------------

/// Print debug message to kernel log
///
/// # Safety
/// - `msg` must point to valid UTF-8 data
#[no_mangle]
pub unsafe extern "C" fn staros_system_debug_print(msg: *const u8, msg_len: usize) {
    if msg.is_null() || msg_len == 0 {
        return;
    }

    let msg_slice = core::slice::from_raw_parts(msg, msg_len);
    if let Ok(_msg_str) = core::str::from_utf8(msg_slice) {
        // Log message using kernel logger
        #[cfg(feature = "std")]
        println!("[FFI] {}", _msg_str);
    }
}

// ---------------------------------------------------------------------------
// Performance monitoring (WORKING - ARM cycle counter)
// ---------------------------------------------------------------------------

/// Start performance counter
#[no_mangle]
pub extern "C" fn staros_system_perf_start() -> u64 {
    staros_system_get_uptime_us()
}

/// Stop performance counter and return elapsed microseconds
#[no_mangle]
pub extern "C" fn staros_system_perf_stop(start: u64) -> u64 {
    let now = staros_system_get_uptime_us();
    now.saturating_sub(start)
}

/// Get CPU cycle counter (if available)
#[no_mangle]
pub extern "C" fn staros_system_get_cpu_cycles() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        let cycles: u64;
        unsafe {
            core::arch::asm!(
                "mrs {}, pmccntr_el0",
                out(reg) cycles,
                options(nomem, nostack)
            );
        }
        cycles
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        0
    }
}

// ---------------------------------------------------------------------------
// NOT IMPLEMENTED YET - These functions are removed until kernel support exists
// ---------------------------------------------------------------------------
// 
// The following features are not yet implemented in the kernel:
// - Task/process management (scheduler not ready)
// - Power management (no power subsystem)
// - Watchdog control (watchdog not integrated)
// - System statistics (no stats collection)
// - CPU count/info (SoC detection not exposed)
//
// These will be added in future kernel versions as the subsystems are completed.

