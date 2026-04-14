//! ARM Generic Timer
//!
//! System timer for scheduling and timekeeping

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::KernelError;

/// Timer frequency in Hz
pub const TIMER_FREQ: u64 = 1_000_000; // 1 MHz

/// Timer tick duration in nanoseconds
pub const TICK_NS: u64 = 1_000_000_000 / TIMER_FREQ;

/// System timer
pub struct Timer {
    ticks: AtomicU64,
    frequency: u64,
}

impl Timer {
    pub const fn new(frequency: u64) -> Self {
        Self {
            ticks: AtomicU64::new(0),
            frequency,
        }
    }

    /// Initialize timer
    pub fn init(&self) -> Result<(), KernelError> {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // Set timer frequency
            core::arch::asm!(
                "msr CNTFRQ_EL0, {freq}",
                freq = in(reg) self.frequency,
            );

            // Enable timer
            self.enable();
        }

        Ok(())
    }

    /// Enable timer
    #[cfg(target_arch = "aarch64")]
    pub fn enable(&self) {
        unsafe {
            // Enable timer interrupt
            core::arch::asm!(
                "msr CNTP_CTL_EL0, {val}",
                val = in(reg) 1u64, // Enable bit
            );
        }
    }

    /// Disable timer
    #[cfg(target_arch = "aarch64")]
    pub fn disable(&self) {
        unsafe {
            core::arch::asm!(
                "msr CNTP_CTL_EL0, {val}",
                val = in(reg) 0u64,
            );
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    pub fn enable(&self) {}

    #[cfg(not(target_arch = "aarch64"))]
    pub fn disable(&self) {}

    /// Set timer compare value (when to fire next interrupt)
    #[cfg(target_arch = "aarch64")]
    pub fn set_compare(&self, ticks: u64) {
        unsafe {
            core::arch::asm!(
                "msr CNTP_CVAL_EL0, {val}",
                val = in(reg) ticks,
            );
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    pub fn set_compare(&self, _ticks: u64) {}

    /// Get current timer value
    #[cfg(target_arch = "aarch64")]
    pub fn current(&self) -> u64 {
        let val: u64;
        unsafe {
            core::arch::asm!(
                "mrs {val}, CNTPCT_EL0",
                val = out(reg) val,
            );
        }
        val
    }

    #[cfg(not(target_arch = "aarch64"))]
    pub fn current(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }

    /// Handle timer interrupt (called from IRQ handler)
    pub fn tick(&self) {
        let ticks = self.ticks.fetch_add(1, Ordering::Relaxed);
        
        // Set next interrupt
        let next = self.current() + (self.frequency / 100); // 10ms
        self.set_compare(next);
    }

    /// Get total ticks since boot
    pub fn ticks(&self) -> u64 {
        self.ticks.load(Ordering::Relaxed)
    }

    /// Get time in nanoseconds
    pub fn nanos(&self) -> u64 {
        let ticks = self.ticks();
        (ticks * 1_000_000_000) / self.frequency
    }

    /// Get time in microseconds
    pub fn micros(&self) -> u64 {
        let ticks = self.ticks();
        (ticks * 1_000_000) / self.frequency
    }

    /// Get time in milliseconds
    pub fn millis(&self) -> u64 {
        let ticks = self.ticks();
        (ticks * 1_000) / self.frequency
    }

    /// Sleep for given number of ticks
    pub fn sleep_ticks(&self, ticks: u64) {
        let start = self.ticks();
        while self.ticks() - start < ticks {
            #[cfg(target_arch = "aarch64")]
            unsafe { core::arch::asm!("wfe") }; // Wait for event
            
            core::hint::spin_loop();
        }
    }

    /// Sleep for given number of microseconds
    pub fn sleep_micros(&self, micros: u64) {
        let ticks = (micros * self.frequency) / 1_000_000;
        self.sleep_ticks(ticks);
    }

    /// Sleep for given number of milliseconds
    pub fn sleep_millis(&self, millis: u64) {
        let ticks = (millis * self.frequency) / 1_000;
        self.sleep_ticks(ticks);
    }
}

unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_creation() {
        let timer = Timer::new(TIMER_FREQ);
        assert_eq!(timer.ticks(), 0);
    }

    #[test]
    fn test_timer_tick() {
        let timer = Timer::new(TIMER_FREQ);
        
        assert_eq!(timer.ticks(), 0);
        
        timer.tick();
        assert_eq!(timer.ticks(), 1);
        
        timer.tick();
        assert_eq!(timer.ticks(), 2);
    }

    #[test]
    fn test_time_conversion() {
        let timer = Timer::new(1_000_000); // 1 MHz
        
        // Simulate 1000 ticks = 1ms
        for _ in 0..1000 {
            timer.tick();
        }
        
        assert_eq!(timer.millis(), 1);
        assert_eq!(timer.micros(), 1000);
    }

    #[test]
    fn test_timer_frequency() {
        let timer = Timer::new(2_000_000); // 2 MHz
        
        for _ in 0..2000 {
            timer.tick();
        }
        
        assert_eq!(timer.millis(), 1);
    }
}
