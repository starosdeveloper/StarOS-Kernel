//! Kernel Logging Framework (klog)
//!
//! Provides structured logging with levels, module context, and a ring buffer
//! for post-mortem analysis. Output goes to UART in early boot, then to the
//! ring buffer once memory is available.

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// Log levels (lower = more critical)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    /// System is unusable
    Emergency = 0,
    /// Action must be taken immediately
    Alert = 1,
    /// Critical conditions
    Critical = 2,
    /// Error conditions
    Error = 3,
    /// Warning conditions
    Warning = 4,
    /// Normal but significant
    Notice = 5,
    /// Informational
    Info = 6,
    /// Debug-level messages
    Debug = 7,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Emergency => "EMERG",
            Self::Alert     => "ALERT",
            Self::Critical  => "CRIT ",
            Self::Error     => "ERROR",
            Self::Warning   => "WARN ",
            Self::Notice    => "NOTE ",
            Self::Info      => "INFO ",
            Self::Debug     => "DEBUG",
        }
    }
}

/// Ring buffer entry for log storage
#[derive(Clone, Copy)]
pub struct LogEntry {
    pub level: Level,
    pub timestamp_us: u64,
    pub module: &'static str,
    pub message: [u8; 120],
    pub msg_len: u8,
}

impl LogEntry {
    const fn empty() -> Self {
        Self {
            level: Level::Debug,
            timestamp_us: 0,
            module: "",
            message: [0; 120],
            msg_len: 0,
        }
    }
}

/// Ring buffer for kernel log messages
const LOG_BUFFER_SIZE: usize = 256;

static mut LOG_BUFFER: [LogEntry; LOG_BUFFER_SIZE] = [LogEntry::empty(); LOG_BUFFER_SIZE];
static LOG_WRITE_POS: AtomicUsize = AtomicUsize::new(0);
static LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
static LOG_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// Set the minimum log level (messages below this level are discarded)
pub fn set_level(level: Level) {
    LOG_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Get current log level
pub fn current_level() -> Level {
    match LOG_LEVEL.load(Ordering::Relaxed) {
        0 => Level::Emergency,
        1 => Level::Alert,
        2 => Level::Critical,
        3 => Level::Error,
        4 => Level::Warning,
        5 => Level::Notice,
        6 => Level::Info,
        _ => Level::Debug,
    }
}

/// Log a message at the given level
pub fn log(level: Level, module: &'static str, msg: &[u8]) {
    if (level as u8) > LOG_LEVEL.load(Ordering::Relaxed) {
        return;
    }

    let pos = LOG_WRITE_POS.fetch_add(1, Ordering::AcqRel) % LOG_BUFFER_SIZE;
    LOG_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut entry = LogEntry::empty();
    entry.level = level;
    entry.module = module;
    entry.timestamp_us = get_timestamp_us();

    let copy_len = msg.len().min(120);
    entry.message[..copy_len].copy_from_slice(&msg[..copy_len]);
    entry.msg_len = copy_len as u8;

    // SAFETY: LOG_BUFFER is only written at unique positions via atomic fetch_add
    unsafe {
        LOG_BUFFER[pos] = entry;
    }

    // Also output to UART for early boot visibility
    #[cfg(not(feature = "std"))]
    uart_output(level, module, &msg[..copy_len]);
}

/// Get total number of log messages written
pub fn total_messages() -> usize {
    LOG_COUNT.load(Ordering::Relaxed)
}

/// Read the last N log entries (for post-mortem analysis)
pub fn read_last(count: usize) -> impl Iterator<Item = &'static LogEntry> {
    let total = LOG_COUNT.load(Ordering::Acquire);
    let available = total.min(LOG_BUFFER_SIZE);
    let read_count = count.min(available);
    let write_pos = LOG_WRITE_POS.load(Ordering::Acquire);

    let start = if write_pos >= read_count {
        write_pos - read_count
    } else {
        LOG_BUFFER_SIZE - (read_count - write_pos)
    };

    (0..read_count).map(move |i| {
        let idx = (start + i) % LOG_BUFFER_SIZE;
        // SAFETY: idx is bounded by LOG_BUFFER_SIZE, entries are Copy
        unsafe { &LOG_BUFFER[idx] }
    })
}

#[cfg(not(feature = "std"))]
fn uart_output(_level: Level, _module: &'static str, _msg: &[u8]) {
    // Will be connected to UART driver once available
}

fn get_timestamp_us() -> u64 {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let cnt: u64;
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nostack, nomem));
        // Assume 1MHz counter (typical for ARM generic timer)
        cnt
    }
    #[cfg(not(target_arch = "aarch64"))]
    { 0 }
}

/// Convenience macros
#[macro_export]
macro_rules! klog_error {
    ($module:expr, $msg:expr) => {
        $crate::klog::log($crate::klog::Level::Error, $module, $msg.as_bytes())
    };
}

#[macro_export]
macro_rules! klog_warn {
    ($module:expr, $msg:expr) => {
        $crate::klog::log($crate::klog::Level::Warning, $module, $msg.as_bytes())
    };
}

#[macro_export]
macro_rules! klog_info {
    ($module:expr, $msg:expr) => {
        $crate::klog::log($crate::klog::Level::Info, $module, $msg.as_bytes())
    };
}

#[macro_export]
macro_rules! klog_debug {
    ($module:expr, $msg:expr) => {
        $crate::klog::log($crate::klog::Level::Debug, $module, $msg.as_bytes())
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_levels() {
        set_level(Level::Debug);
        let before = total_messages();
        log(Level::Info, "test", b"hello world");
        assert!(total_messages() > before);
    }

    #[test]
    fn test_level_filtering() {
        set_level(Level::Error);
        let before = total_messages();
        log(Level::Debug, "test", b"should be filtered");
        assert_eq!(total_messages(), before); // Debug filtered at Error level
        log(Level::Error, "test", b"should pass");
        assert_eq!(total_messages(), before + 1);
        // Reset for other tests
        set_level(Level::Debug);
    }
}
