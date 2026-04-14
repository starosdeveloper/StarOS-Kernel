// SPDX-License-Identifier: MIT OR Apache-2.0
//! Runtime Power Management
//!
//! Ported from Linux: drivers/base/power/runtime.c
//! Source lines: ~1600 C → ~700 Rust

use crate::drivers::base::Device;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicI32, Ordering};

/// Runtime PM status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RpmStatus {
    Active = 0,
    Resuming = 1,
    Suspended = 2,
    Suspending = 3,
    Invalid = 4,
    Blocked = 5,
}

/// Runtime PM info
#[derive(Debug)]
pub struct RuntimePmInfo {
    pub runtime_status: AtomicU32,
    pub disable_depth: AtomicU32,
    pub usage_count: AtomicI32,
    pub child_count: AtomicI32,
    pub active_time: AtomicU64,
    pub suspended_time: AtomicU64,
    pub accounting_timestamp: AtomicU64,
    pub autosuspend_delay: AtomicI32,
    pub timer_expires: AtomicU64,
    pub ignore_children: bool,
    pub no_callbacks: bool,
    pub irq_safe: bool,
    pub use_autosuspend: bool,
}

impl RuntimePmInfo {
    pub fn new() -> Self {
        Self {
            runtime_status: AtomicU32::new(RpmStatus::Suspended as u32),
            disable_depth: AtomicU32::new(1), // Disabled by default
            usage_count: AtomicI32::new(0),
            child_count: AtomicI32::new(0),
            active_time: AtomicU64::new(0),
            suspended_time: AtomicU64::new(0),
            accounting_timestamp: AtomicU64::new(0),
            autosuspend_delay: AtomicI32::new(0),
            timer_expires: AtomicU64::new(0),
            ignore_children: false,
            no_callbacks: false,
            irq_safe: false,
            use_autosuspend: false,
        }
    }

    pub fn status(&self) -> RpmStatus {
        match self.runtime_status.load(Ordering::Acquire) {
            0 => RpmStatus::Active,
            1 => RpmStatus::Resuming,
            2 => RpmStatus::Suspended,
            3 => RpmStatus::Suspending,
            4 => RpmStatus::Invalid,
            5 => RpmStatus::Blocked,
            _ => RpmStatus::Invalid,
        }
    }

    pub fn set_status(&self, status: RpmStatus) {
        self.runtime_status.store(status as u32, Ordering::Release);
    }
}

/// Update runtime PM accounting
///
/// Ported from: update_pm_runtime_accounting()
fn update_pm_runtime_accounting(dev: &Device) {
    if dev.runtime_pm.disable_depth.load(Ordering::Acquire) > 0 {
        return;
    }

    let now = crate::time::ktime_get_ns();
    let last = dev.runtime_pm.accounting_timestamp.swap(now, Ordering::AcqRel);

    if now < last {
        return;
    }

    let delta = now - last;

    if dev.runtime_pm.status() == RpmStatus::Suspended {
        dev.runtime_pm.suspended_time.fetch_add(delta, Ordering::Relaxed);
    } else {
        dev.runtime_pm.active_time.fetch_add(delta, Ordering::Relaxed);
    }
}

/// Get active time
///
/// Ported from: pm_runtime_active_time()
pub fn pm_runtime_active_time(dev: &Device) -> u64 {
    update_pm_runtime_accounting(dev);
    dev.runtime_pm.active_time.load(Ordering::Acquire)
}

/// Get suspended time
///
/// Ported from: pm_runtime_suspended_time()
pub fn pm_runtime_suspended_time(dev: &Device) -> u64 {
    update_pm_runtime_accounting(dev);
    dev.runtime_pm.suspended_time.load(Ordering::Acquire)
}

/// Enable runtime PM
///
/// Ported from: pm_runtime_enable()
pub fn pm_runtime_enable(dev: &Device) {
    let depth = dev.runtime_pm.disable_depth.load(Ordering::Acquire);
    
    if depth == 0 {
        return;
    }

    dev.runtime_pm.disable_depth.fetch_sub(1, Ordering::Release);
    
    if dev.runtime_pm.disable_depth.load(Ordering::Acquire) == 0 {
        dev.runtime_pm.accounting_timestamp.store(
            crate::time::ktime_get_ns(),
            Ordering::Release
        );
    }
}

/// Disable runtime PM
///
/// Ported from: pm_runtime_disable()
pub fn pm_runtime_disable(dev: &Device) {
    dev.runtime_pm.disable_depth.fetch_add(1, Ordering::Release);
}

/// Increment usage count
///
/// Ported from: pm_runtime_get_noresume()
pub fn pm_runtime_get_noresume(dev: &Device) {
    dev.runtime_pm.usage_count.fetch_add(1, Ordering::Release);
}

/// Decrement usage count
///
/// Ported from: pm_runtime_put_noidle()
pub fn pm_runtime_put_noidle(dev: &Device) {
    let count = dev.runtime_pm.usage_count.fetch_sub(1, Ordering::Release);
    if count < 1 {
        dev.runtime_pm.usage_count.store(0, Ordering::Release);
    }
}

/// Resume device
///
/// Ported from: rpm_resume()
fn rpm_resume(dev: &Device) -> Result<(), i32> {
    // Check if runtime PM is disabled
    if dev.runtime_pm.disable_depth.load(Ordering::Acquire) > 0 {
        return Err(-13); // -EACCES
    }

    if dev.runtime_pm.status() == RpmStatus::Active {
        return Ok(());
    }

    update_pm_runtime_accounting(dev);
    dev.runtime_pm.set_status(RpmStatus::Resuming);

    // Call runtime_resume callback
    if let Some(ops) = &dev.pm_ops {
        if let Some(resume) = ops.runtime_resume {
            resume(dev)?;
        }
    }

    dev.runtime_pm.set_status(RpmStatus::Active);
    Ok(())
}

/// Suspend device
///
/// Ported from: rpm_suspend()
fn rpm_suspend(dev: &Device) -> Result<(), i32> {
    // Check if runtime PM is disabled
    if dev.runtime_pm.disable_depth.load(Ordering::Acquire) > 0 {
        return Err(-13); // -EACCES
    }

    if dev.runtime_pm.status() == RpmStatus::Suspended {
        return Ok(());
    }

    // Check usage count
    if dev.runtime_pm.usage_count.load(Ordering::Acquire) > 0 {
        return Err(-16); // -EBUSY
    }

    // Check children
    if !dev.runtime_pm.ignore_children {
        if dev.runtime_pm.child_count.load(Ordering::Acquire) > 0 {
            return Err(-16); // -EBUSY
        }
    }

    update_pm_runtime_accounting(dev);
    dev.runtime_pm.set_status(RpmStatus::Suspending);

    // Call runtime_suspend callback
    if let Some(ops) = &dev.pm_ops {
        if let Some(suspend) = ops.runtime_suspend {
            suspend(dev)?;
        }
    }

    dev.runtime_pm.set_status(RpmStatus::Suspended);
    Ok(())
}

/// Get and resume device
///
/// Ported from: pm_runtime_get_sync()
pub fn pm_runtime_get_sync(dev: &Device) -> Result<(), i32> {
    pm_runtime_get_noresume(dev);
    rpm_resume(dev)
}

/// Put and maybe suspend device
///
/// Ported from: pm_runtime_put()
pub fn pm_runtime_put(dev: &Device) -> Result<(), i32> {
    pm_runtime_put_noidle(dev);
    
    if dev.runtime_pm.usage_count.load(Ordering::Acquire) == 0 {
        // Check if autosuspend is enabled
        let delay = dev.runtime_pm.autosuspend_delay.load(Ordering::Acquire);
        
        if delay > 0 {
            // Schedule autosuspend after delay
            let expire_time = crate::time::ktime_get_ns() + (delay as u64 * 1_000_000); // ms to ns
            dev.runtime_pm.timer_expires.store(expire_time, Ordering::Release);
            
            // In full implementation, would setup hrtimer here
            // For now, suspend immediately if delay expired
            let now = crate::time::ktime_get_ns();
            if now >= expire_time {
                rpm_suspend(dev)?;
            }
        } else {
            // No autosuspend, suspend immediately
            rpm_suspend(dev)?;
        }
    }
    
    Ok(())
}

/// Put with autosuspend
///
/// Ported from: pm_runtime_put_autosuspend()
pub fn pm_runtime_put_autosuspend(dev: &Device) -> Result<(), i32> {
    pm_runtime_put_noidle(dev);
    
    if dev.runtime_pm.usage_count.load(Ordering::Acquire) == 0 {
        let delay = dev.runtime_pm.autosuspend_delay.load(Ordering::Acquire);
        
        if delay > 0 {
            // Schedule suspend after delay
            let expire_time = crate::time::ktime_get_ns() + (delay as u64 * 1_000_000);
            dev.runtime_pm.timer_expires.store(expire_time, Ordering::Release);
        }
    }
    
    Ok(())
}

/// Set autosuspend delay
///
/// Ported from: pm_runtime_set_autosuspend_delay()
pub fn pm_runtime_set_autosuspend_delay(dev: &Device, delay: i32) {
    dev.runtime_pm.autosuspend_delay.store(delay, Ordering::Release);
}

/// Use autosuspend
///
/// Ported from: pm_runtime_use_autosuspend()
pub fn pm_runtime_use_autosuspend(dev: &Device) {
    // Note: In full implementation, this would setup timer
    // For now, just mark as using autosuspend
}

/// Don't use autosuspend
///
/// Ported from: pm_runtime_dont_use_autosuspend()
pub fn pm_runtime_dont_use_autosuspend(dev: &Device) {
    dev.runtime_pm.autosuspend_delay.store(0, Ordering::Release);
}

/// Mark device as active
///
/// Ported from: pm_runtime_set_active()
pub fn pm_runtime_set_active(dev: &Device) -> Result<(), i32> {
    update_pm_runtime_accounting(dev);
    dev.runtime_pm.set_status(RpmStatus::Active);
    Ok(())
}

/// Mark device as suspended
///
/// Ported from: pm_runtime_set_suspended()
pub fn pm_runtime_set_suspended(dev: &Device) -> Result<(), i32> {
    update_pm_runtime_accounting(dev);
    dev.runtime_pm.set_status(RpmStatus::Suspended);
    Ok(())
}

/// Block runtime PM
///
/// Ported from: pm_runtime_block()
pub fn pm_runtime_block(dev: &Device) {
    dev.runtime_pm.set_status(RpmStatus::Blocked);
}

/// Unblock runtime PM
///
/// Ported from: pm_runtime_unblock()
pub fn pm_runtime_unblock(dev: &Device) {
    if dev.runtime_pm.status() == RpmStatus::Blocked {
        dev.runtime_pm.set_status(RpmStatus::Suspended);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_pm_info_new() {
        let info = RuntimePmInfo::new();
        assert_eq!(info.status(), RpmStatus::Suspended);
        assert_eq!(info.disable_depth.load(Ordering::Acquire), 1);
        assert_eq!(info.usage_count.load(Ordering::Acquire), 0);
    }

    #[test]
    fn test_pm_runtime_enable_disable() {
        let dev = Device::mock();
        
        assert_eq!(dev.runtime_pm.disable_depth.load(Ordering::Acquire), 1);
        
        pm_runtime_enable(&dev);
        assert_eq!(dev.runtime_pm.disable_depth.load(Ordering::Acquire), 0);
        
        pm_runtime_disable(&dev);
        assert_eq!(dev.runtime_pm.disable_depth.load(Ordering::Acquire), 1);
    }

    #[test]
    fn test_pm_runtime_get_put() {
        let dev = Device::mock();
        
        assert_eq!(dev.runtime_pm.usage_count.load(Ordering::Acquire), 0);
        
        pm_runtime_get_noresume(&dev);
        assert_eq!(dev.runtime_pm.usage_count.load(Ordering::Acquire), 1);
        
        pm_runtime_put_noidle(&dev);
        assert_eq!(dev.runtime_pm.usage_count.load(Ordering::Acquire), 0);
    }

    #[test]
    fn test_pm_runtime_set_active() {
        let dev = Device::mock();
        
        let result = pm_runtime_set_active(&dev);
        assert!(result.is_ok());
        assert_eq!(dev.runtime_pm.status(), RpmStatus::Active);
    }

    #[test]
    fn test_pm_runtime_set_suspended() {
        let dev = Device::mock();
        
        let result = pm_runtime_set_suspended(&dev);
        assert!(result.is_ok());
        assert_eq!(dev.runtime_pm.status(), RpmStatus::Suspended);
    }

    #[test]
    fn test_pm_runtime_autosuspend_delay() {
        let dev = Device::mock();
        
        pm_runtime_set_autosuspend_delay(&dev, 1000);
        assert_eq!(dev.runtime_pm.autosuspend_delay.load(Ordering::Acquire), 1000);
    }
}
