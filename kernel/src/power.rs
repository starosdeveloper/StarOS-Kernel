//! Kernel Power Management Framework
//!
//! Provides CPU frequency scaling, sleep states, and device power management.
//! Designed for ARM64 mobile SoCs with big.LITTLE architectures.

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU8, AtomicU32, AtomicBool, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicBool, Ordering};

use crate::error::KernelError;

/// System power states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerState {
    /// Full performance, all cores active
    Active = 0,
    /// Reduced frequency, some cores may be parked
    Idle = 1,
    /// Light sleep - fast wake, clocks gated
    Standby = 2,
    /// Deep sleep - slow wake, power domains off
    Suspend = 3,
    /// Hibernate - RAM contents saved to storage
    Hibernate = 4,
    /// Power off
    Off = 5,
}

/// CPU frequency operating point
#[derive(Debug, Clone, Copy)]
pub struct OppEntry {
    pub freq_khz: u32,
    pub voltage_uv: u32,
}

/// CPU performance governor policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Governor {
    /// Always maximum frequency
    Performance = 0,
    /// Always minimum frequency
    Powersave = 1,
    /// Scale based on load
    OnDemand = 2,
    /// User-specified frequency
    Userspace = 3,
}

/// Power management state
pub struct PowerManager {
    state: AtomicU8,
    governor: AtomicU8,
    current_freq_khz: AtomicU32,
    min_freq_khz: u32,
    max_freq_khz: u32,
    suspend_blocked: AtomicBool,
    wake_sources: AtomicU32,
}

impl PowerManager {
    pub const fn new(min_freq_khz: u32, max_freq_khz: u32) -> Self {
        Self {
            state: AtomicU8::new(PowerState::Active as u8),
            governor: AtomicU8::new(Governor::OnDemand as u8),
            current_freq_khz: AtomicU32::new(max_freq_khz),
            min_freq_khz,
            max_freq_khz,
            suspend_blocked: AtomicBool::new(false),
            wake_sources: AtomicU32::new(0),
        }
    }

    /// Get current power state
    pub fn state(&self) -> PowerState {
        match self.state.load(Ordering::Acquire) {
            0 => PowerState::Active,
            1 => PowerState::Idle,
            2 => PowerState::Standby,
            3 => PowerState::Suspend,
            4 => PowerState::Hibernate,
            _ => PowerState::Off,
        }
    }

    /// Request transition to a new power state
    pub fn set_state(&self, new_state: PowerState) -> Result<(), KernelError> {
        let current = self.state();

        // Can't suspend if blocked
        if matches!(new_state, PowerState::Suspend | PowerState::Hibernate)
            && self.suspend_blocked.load(Ordering::Acquire)
        {
            return Err(KernelError::OperationFailed);
        }

        // Validate transition
        match (current, new_state) {
            (PowerState::Off, _) => return Err(KernelError::NotSupported),
            (_, PowerState::Off) => {
                // Shutdown sequence
                self.state.store(PowerState::Off as u8, Ordering::Release);
                self.shutdown_sequence();
            }
            _ => {
                self.state.store(new_state as u8, Ordering::Release);
            }
        }

        Ok(())
    }

    /// Set CPU frequency governor
    pub fn set_governor(&self, gov: Governor) {
        self.governor.store(gov as u8, Ordering::Release);
    }

    /// Get current governor
    pub fn governor(&self) -> Governor {
        match self.governor.load(Ordering::Acquire) {
            0 => Governor::Performance,
            1 => Governor::Powersave,
            2 => Governor::OnDemand,
            _ => Governor::Userspace,
        }
    }

    /// Set CPU frequency (in kHz)
    pub fn set_frequency(&self, freq_khz: u32) -> Result<(), KernelError> {
        if freq_khz < self.min_freq_khz || freq_khz > self.max_freq_khz {
            return Err(KernelError::InvalidParameter("frequency out of range"));
        }

        self.current_freq_khz.store(freq_khz, Ordering::Release);

        // Apply to hardware
        #[cfg(target_arch = "aarch64")]
        self.apply_frequency(freq_khz);

        Ok(())
    }

    /// Get current CPU frequency
    pub fn frequency_khz(&self) -> u32 {
        self.current_freq_khz.load(Ordering::Acquire)
    }

    /// Block suspend (e.g., during DMA transfer)
    pub fn block_suspend(&self) {
        self.suspend_blocked.store(true, Ordering::Release);
    }

    /// Unblock suspend
    pub fn unblock_suspend(&self) {
        self.suspend_blocked.store(false, Ordering::Release);
    }

    /// Register a wake source (IRQ number)
    pub fn register_wake_source(&self, irq: u8) {
        self.wake_sources.fetch_or(1 << (irq & 31), Ordering::AcqRel);
    }

    /// Called by scheduler when CPU is idle - scale down frequency
    pub fn cpu_idle_enter(&self) {
        if self.governor() == Governor::OnDemand {
            let current = self.current_freq_khz.load(Ordering::Relaxed);
            let target = (current / 2).max(self.min_freq_khz);
            self.current_freq_khz.store(target, Ordering::Relaxed);
        }
    }

    /// Called by scheduler when CPU exits idle - scale up frequency
    pub fn cpu_idle_exit(&self) {
        if self.governor() == Governor::OnDemand {
            self.current_freq_khz.store(self.max_freq_khz, Ordering::Relaxed);
        }
    }

    #[cfg(target_arch = "aarch64")]
    fn apply_frequency(&self, _freq_khz: u32) {
        // Would write to SoC-specific clock registers
    }

    fn shutdown_sequence(&self) {
        // Flush caches, disable interrupts, power off
        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!(
                "msr daifset, #0xf",  // Disable all interrupts
                "dsb sy",              // Data sync barrier
                "isb",                 // Instruction sync
                options(nostack)
            );
        }
    }
}

unsafe impl Send for PowerManager {}
unsafe impl Sync for PowerManager {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_state_transitions() {
        let pm = PowerManager::new(300_000, 2_400_000);
        assert_eq!(pm.state(), PowerState::Active);

        pm.set_state(PowerState::Idle).unwrap();
        assert_eq!(pm.state(), PowerState::Idle);

        pm.set_state(PowerState::Suspend).unwrap();
        assert_eq!(pm.state(), PowerState::Suspend);
    }

    #[test]
    fn test_suspend_blocking() {
        let pm = PowerManager::new(300_000, 2_400_000);
        pm.block_suspend();
        assert!(pm.set_state(PowerState::Suspend).is_err());
        pm.unblock_suspend();
        assert!(pm.set_state(PowerState::Suspend).is_ok());
    }

    #[test]
    fn test_frequency_scaling() {
        let pm = PowerManager::new(300_000, 2_400_000);
        assert_eq!(pm.frequency_khz(), 2_400_000);

        pm.set_frequency(1_200_000).unwrap();
        assert_eq!(pm.frequency_khz(), 1_200_000);

        // Out of range
        assert!(pm.set_frequency(100_000).is_err());
        assert!(pm.set_frequency(5_000_000).is_err());
    }

    #[test]
    fn test_governor() {
        let pm = PowerManager::new(300_000, 2_400_000);
        pm.set_governor(Governor::Powersave);
        assert_eq!(pm.governor(), Governor::Powersave);
    }

    #[test]
    fn test_idle_scaling() {
        let pm = PowerManager::new(300_000, 2_400_000);
        pm.set_governor(Governor::OnDemand);

        pm.cpu_idle_enter();
        assert!(pm.frequency_khz() < 2_400_000);

        pm.cpu_idle_exit();
        assert_eq!(pm.frequency_khz(), 2_400_000);
    }
}
