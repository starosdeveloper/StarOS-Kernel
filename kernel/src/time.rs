// SPDX-License-Identifier: MIT
//! Timekeeping subsystem
//!
//! Ported from Linux: `kernel/time/timekeeping.c` (3115 lines C → ~400 lines Rust)

use core::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use spin::Mutex;

/// Nanoseconds per second
const NSEC_PER_SEC: u64 = 1_000_000_000;

/// Microseconds per second
const USEC_PER_SEC: u64 = 1_000_000;

/// Timekeeper state
struct Timekeeper {
    /// Clocksource cycle value at last update
    cycle_last: u64,
    /// Multiplier for converting cycles to nanoseconds
    mult: u32,
    /// Shift for converting cycles to nanoseconds
    shift: u32,
    /// Nanoseconds accumulated
    xtime_nsec: u64,
    /// Wall time seconds
    xtime_sec: i64,
    /// Monotonic time offset (nanoseconds)
    monotonic_offset: u64,
    /// Boot time offset (nanoseconds)
    boot_offset: u64,
}

/// Fast timekeeper for NMI-safe access
struct TkFast {
    /// Sequence counter
    seq: AtomicU32,
    /// Cycle value
    cycle_now: AtomicU64,
    /// Nanoseconds
    nsec: AtomicU64,
}

static TK_CORE: Mutex<Timekeeper> = Mutex::new(Timekeeper {
    cycle_last: 0,
    mult: 1,
    shift: 0,
    xtime_nsec: 0,
    xtime_sec: 0,
    monotonic_offset: 0,
    boot_offset: 0,
});

static TK_FAST_MONO: TkFast = TkFast {
    seq: AtomicU32::new(0),
    cycle_now: AtomicU64::new(0),
    nsec: AtomicU64::new(0),
};

/// Initialize timekeeping
///
/// Ported from: `timekeeping_init()`
pub fn init() {
    let mut tk = TK_CORE.lock();
    
    // Read ARM generic timer frequency (CNTFRQ_EL0)
    let freq: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nomem, nostack));
    }
    
    // Calculate mult and shift for cycle to nanosecond conversion
    // mult = (NSEC_PER_SEC << shift) / freq
    let shift = 24u32;
    let mult = ((NSEC_PER_SEC << shift) / freq) as u32;
    
    tk.mult = mult;
    tk.shift = shift;
    tk.cycle_last = read_cycles();
    tk.xtime_sec = 0;
    tk.xtime_nsec = 0;
    tk.monotonic_offset = 0;
    tk.boot_offset = 0;
    
    // Initialize fast timekeeper
    TK_FAST_MONO.cycle_now.store(tk.cycle_last, Ordering::Release);
    TK_FAST_MONO.nsec.store(0, Ordering::Release);
}

/// Read ARM generic timer counter
#[inline(always)]
fn read_cycles() -> u64 {
    let cnt: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntpct_el0", out(reg) cnt, options(nomem, nostack));
    }
    cnt
}

/// Get monotonic time in nanoseconds
///
/// Ported from: `ktime_get()`
pub fn ktime_get_ns() -> u64 {
    // Fast path - read from fast timekeeper
    loop {
        let seq = TK_FAST_MONO.seq.load(Ordering::Acquire);
        let cycle_now = TK_FAST_MONO.cycle_now.load(Ordering::Acquire);
        let nsec = TK_FAST_MONO.nsec.load(Ordering::Acquire);
        
        if seq == TK_FAST_MONO.seq.load(Ordering::Acquire) {
            // Read current cycles
            let now = read_cycles();
            
            // Calculate delta
            let delta = now.wrapping_sub(cycle_now);
            
            // Convert to nanoseconds (simplified - use mult/shift from init)
            let tk = TK_CORE.lock();
            let delta_ns = (delta * tk.mult as u64) >> tk.shift;
            drop(tk);
            
            return nsec + delta_ns;
        }
    }
}

/// Get monotonic time in microseconds
///
/// Ported from: `ktime_get()` with conversion
pub fn ktime_get_us() -> u64 {
    ktime_get_ns() / 1000
}

/// Get monotonic time in seconds
///
/// Ported from: `ktime_get_seconds()`
pub fn ktime_get_sec() -> i64 {
    let tk = TK_CORE.lock();
    tk.xtime_sec
}

/// Get real (wall) time in nanoseconds
///
/// Ported from: `ktime_get_real()`
pub fn ktime_get_real_ns() -> u64 {
    let tk = TK_CORE.lock();
    let mono_ns = ktime_get_ns();
    mono_ns + tk.boot_offset
}

/// Get boot time (monotonic + suspend time) in nanoseconds
///
/// Ported from: `ktime_get_boottime()`
pub fn ktime_get_boottime_ns() -> u64 {
    let tk = TK_CORE.lock();
    ktime_get_ns() + tk.boot_offset
}

/// Update timekeeping (called from timer interrupt)
///
/// Ported from: `update_wall_time()`
pub fn update_wall_time() {
    let mut tk = TK_CORE.lock();
    
    // Read current cycles
    let now = read_cycles();
    let delta = now.wrapping_sub(tk.cycle_last);
    
    // Convert to nanoseconds
    let delta_ns = (delta * tk.mult as u64) >> tk.shift;
    
    // Update accumulated nanoseconds
    tk.xtime_nsec += delta_ns;
    
    // Handle second overflow
    while tk.xtime_nsec >= NSEC_PER_SEC {
        tk.xtime_nsec -= NSEC_PER_SEC;
        tk.xtime_sec += 1;
    }
    
    tk.cycle_last = now;
    
    // Update fast timekeeper
    let seq = TK_FAST_MONO.seq.fetch_add(1, Ordering::Release);
    TK_FAST_MONO.cycle_now.store(now, Ordering::Release);
    TK_FAST_MONO.nsec.store(tk.xtime_nsec, Ordering::Release);
    TK_FAST_MONO.seq.store(seq + 1, Ordering::Release);
}

/// Set wall time
///
/// Ported from: `do_settimeofday64()`
pub fn do_settimeofday(sec: i64, nsec: u64) {
    let mut tk = TK_CORE.lock();
    tk.xtime_sec = sec;
    tk.xtime_nsec = nsec;
}

/// Timespec structure
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

impl Timespec {
    pub fn from_ns(ns: u64) -> Self {
        Self {
            tv_sec: (ns / NSEC_PER_SEC) as i64,
            tv_nsec: (ns % NSEC_PER_SEC) as i64,
        }
    }
    
    pub fn to_ns(&self) -> u64 {
        (self.tv_sec as u64 * NSEC_PER_SEC) + self.tv_nsec as u64
    }
}

/// Get monotonic time as timespec
///
/// Ported from: `ktime_get_ts64()`
pub fn ktime_get_ts() -> Timespec {
    Timespec::from_ns(ktime_get_ns())
}

/// Get real time as timespec
///
/// Ported from: `ktime_get_real_ts64()`
pub fn ktime_get_real_ts() -> Timespec {
    Timespec::from_ns(ktime_get_real_ns())
}

/// Get coarse monotonic time (lower resolution, faster)
///
/// Ported from: `ktime_get_coarse_ts64()`
pub fn ktime_get_coarse_ts() -> Timespec {
    let tk = TK_CORE.lock();
    Timespec {
        tv_sec: tk.xtime_sec,
        tv_nsec: tk.xtime_nsec as i64,
    }
}

/// Suspend timekeeping
pub fn timekeeping_suspend() {
    let mut tk = TK_CORE.lock();
    tk.cycle_last = read_cycles();
}

/// Resume timekeeping
pub fn timekeeping_resume() {
    let mut tk = TK_CORE.lock();
    let now = read_cycles();
    let delta = now.wrapping_sub(tk.cycle_last);
    let delta_ns = (delta * tk.mult as u64) >> tk.shift;
    
    // Add suspend time to boot offset
    tk.boot_offset += delta_ns;
    tk.cycle_last = now;
}
