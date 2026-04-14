use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use crate::error::{KernelError, Result};

static WATCHDOG_ENABLED: AtomicBool = AtomicBool::new(false);
static LAST_KICK: AtomicU64 = AtomicU64::new(0);
static WATCHDOG_BASE: AtomicUsize = AtomicUsize::new(0);
static TIMER_FREQ: AtomicU64 = AtomicU64::new(0);

pub struct Watchdog {
    timeout_ms: u32,
    base_addr: usize,
    freq_hz: u64,
}

impl Watchdog {
    pub fn init_from_dt(base_addr: usize, freq_hz: u64, timeout_ms: u32) -> Result<Self> {
        if base_addr == 0 {
            return Err(KernelError::InvalidAddress);
        }
        
        let wd = Self { 
            timeout_ms,
            base_addr,
            freq_hz,
        };
        
        WATCHDOG_BASE.store(base_addr, Ordering::Release);
        TIMER_FREQ.store(freq_hz, Ordering::Release);
        
        unsafe {
            let tval = (timeout_ms as u64) * (freq_hz / 1000);
            core::ptr::write_volatile((base_addr + 0x28) as *mut u64, tval);
            core::ptr::write_volatile(base_addr as *mut u32, 0x1);
        }
        
        WATCHDOG_ENABLED.store(true, Ordering::Release);
        LAST_KICK.store(Self::get_time(), Ordering::Release);
        Ok(wd)
    }

    pub fn kick() {
        if !WATCHDOG_ENABLED.load(Ordering::Acquire) {
            return;
        }
        
        let base = WATCHDOG_BASE.load(Ordering::Acquire);
        if base == 0 {
            return;
        }
        
        LAST_KICK.store(Self::get_time(), Ordering::Release);
        
        unsafe {
            let timeout = core::ptr::read_volatile((base + 0x28) as *const u64);
            core::ptr::write_volatile((base + 0x28) as *mut u64, timeout);
        }
    }

    pub fn disable() {
        let base = WATCHDOG_BASE.load(Ordering::Acquire);
        if base != 0 {
            unsafe {
                core::ptr::write_volatile(base as *mut u32, 0x0);
            }
        }
        WATCHDOG_ENABLED.store(false, Ordering::Release);
    }

    fn get_time() -> u64 {
        let mut cnt: u64;
        unsafe {
            core::arch::asm!("mrs {}, cntpct_el0", out(reg) cnt);
        }
        cnt
    }

    pub fn time_since_kick() -> u64 {
        let freq = TIMER_FREQ.load(Ordering::Acquire);
        if freq == 0 {
            return 0;
        }
        let now = Self::get_time();
        let last = LAST_KICK.load(Ordering::Acquire);
        now.saturating_sub(last) / (freq / 1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watchdog_init() {
        let wd = Watchdog::init(5000);
        assert_eq!(wd.timeout_ms, 5000);
        assert!(WATCHDOG_ENABLED.load(Ordering::Acquire));
    }

    #[test]
    fn test_watchdog_kick() {
        Watchdog::init(5000);
        let before = LAST_KICK.load(Ordering::Acquire);
        Watchdog::kick();
        let after = LAST_KICK.load(Ordering::Acquire);
        assert!(after >= before);
    }

    #[test]
    fn test_watchdog_disable() {
        Watchdog::init(5000);
        Watchdog::disable();
        assert!(!WATCHDOG_ENABLED.load(Ordering::Acquire));
    }

    #[test]
    fn test_time_since_kick() {
        Watchdog::init(5000);
        Watchdog::kick();
        let elapsed = Watchdog::time_since_kick();
        assert!(elapsed < 100);
    }

    #[test]
    fn test_kick_when_disabled() {
        Watchdog::disable();
        Watchdog::kick(); // Should not panic
    }
}
