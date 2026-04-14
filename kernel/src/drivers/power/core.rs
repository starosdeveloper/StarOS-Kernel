// SPDX-License-Identifier: MIT OR Apache-2.0
//! Power Management Core
//!
//! Ported from Linux: drivers/base/power/main.c
//! Source lines: ~1800 C → ~800 Rust

use crate::drivers::base::Device;
use crate::sync::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

/// PM event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PmEvent {
    Suspend = 0x0002,
    Resume = 0x0010,
    Freeze = 0x0001,
    Quiesce = 0x0200,
    Hibernate = 0x0004,
    Thaw = 0x0020,
    Restore = 0x0040,
    Recover = 0x0080,
    Poweroff = 0x0008,
}

impl PmEvent {
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Suspend => "suspend",
            Self::Resume => "resume",
            Self::Freeze => "freeze",
            Self::Quiesce => "quiesce",
            Self::Hibernate => "hibernate",
            Self::Thaw => "thaw",
            Self::Restore => "restore",
            Self::Recover => "recover",
            Self::Poweroff => "poweroff",
        }
    }
}

/// PM message
#[derive(Debug, Clone, Copy)]
pub struct PmMessage {
    pub event: PmEvent,
}

/// Device PM info
#[derive(Debug)]
pub struct DevicePmInfo {
    pub is_prepared: AtomicBool,
    pub is_suspended: AtomicBool,
    pub is_noirq_suspended: AtomicBool,
    pub is_late_suspended: AtomicBool,
    pub async_suspend: bool,
    pub in_dpm_list: AtomicBool,
}

impl DevicePmInfo {
    pub fn new() -> Self {
        Self {
            is_prepared: AtomicBool::new(false),
            is_suspended: AtomicBool::new(false),
            is_noirq_suspended: AtomicBool::new(false),
            is_late_suspended: AtomicBool::new(false),
            async_suspend: false,
            in_dpm_list: AtomicBool::new(false),
        }
    }
}

/// Global DPM list
static DPM_LIST: Mutex<Vec<Device>> = Mutex::new(Vec::new());
static DPM_PREPARED_LIST: Mutex<Vec<Device>> = Mutex::new(Vec::new());
static DPM_SUSPENDED_LIST: Mutex<Vec<Device>> = Mutex::new(Vec::new());

static PM_TRANSITION: Mutex<Option<PmMessage>> = Mutex::new(None);

/// Check if recovering from hibernate error
///
/// Ported from: pm_hibernate_is_recovering()
pub fn pm_hibernate_is_recovering() -> bool {
    if let Some(msg) = *PM_TRANSITION.lock() {
        msg.event == PmEvent::Recover
    } else {
        false
    }
}

/// Initialize device PM fields
///
/// Ported from: device_pm_sleep_init()
pub fn device_pm_sleep_init(dev: &Device) {
    // PM info is initialized in DevicePmInfo::new()
}

/// Lock DPM list
///
/// Ported from: device_pm_lock()
pub fn device_pm_lock() {
    let _lock = DPM_LIST.lock();
}

/// Unlock DPM list
///
/// Ported from: device_pm_unlock()
pub fn device_pm_unlock() {
    drop(DPM_LIST.lock());
}

/// Add device to PM list
///
/// Ported from: device_pm_add()
pub fn device_pm_add(dev: Device) {
    let mut list = DPM_LIST.lock();
    list.push(dev.clone());
    dev.power.in_dpm_list.store(true, Ordering::Release);
}

/// Remove device from PM list
///
/// Ported from: device_pm_remove()
pub fn device_pm_remove(dev: &Device) {
    let mut list = DPM_LIST.lock();
    list.retain(|d| d.id != dev.id);
    dev.power.in_dpm_list.store(false, Ordering::Release);
}

/// Move device before another in PM list
///
/// Ported from: device_pm_move_before()
pub fn device_pm_move_before(deva: &Device, devb: &Device) {
    let mut list = DPM_LIST.lock();
    
    if let Some(pos_a) = list.iter().position(|d| d.id == deva.id) {
        if let Some(pos_b) = list.iter().position(|d| d.id == devb.id) {
            let dev = list.remove(pos_a);
            let new_pos = if pos_a < pos_b { pos_b - 1 } else { pos_b };
            list.insert(new_pos, dev);
        }
    }
}

/// Move device after another in PM list
///
/// Ported from: device_pm_move_after()
pub fn device_pm_move_after(deva: &Device, devb: &Device) {
    let mut list = DPM_LIST.lock();
    
    if let Some(pos_a) = list.iter().position(|d| d.id == deva.id) {
        if let Some(pos_b) = list.iter().position(|d| d.id == devb.id) {
            let dev = list.remove(pos_a);
            let new_pos = if pos_a < pos_b { pos_b } else { pos_b + 1 };
            list.insert(new_pos, dev);
        }
    }
}

/// Move device to end of PM list
///
/// Ported from: device_pm_move_last()
pub fn device_pm_move_last(dev: &Device) {
    let mut list = DPM_LIST.lock();
    
    if let Some(pos) = list.iter().position(|d| d.id == dev.id) {
        let device = list.remove(pos);
        list.push(device);
    }
}

/// Suspend a single device
///
/// Ported from: device_suspend()
fn device_suspend(dev: &Device, state: PmMessage) -> Result<(), i32> {
    // Call suspend callback if available
    if let Some(ops) = &dev.pm_ops {
        if let Some(suspend) = ops.suspend {
            suspend(dev)?;
        }
    }
    
    dev.power.is_suspended.store(true, Ordering::Release);
    Ok(())
}

/// Resume a single device
///
/// Ported from: device_resume()
fn device_resume(dev: &Device, state: PmMessage) -> Result<(), i32> {
    // Call resume callback if available
    if let Some(ops) = &dev.pm_ops {
        if let Some(resume) = ops.resume {
            resume(dev)?;
        }
    }
    
    dev.power.is_suspended.store(false, Ordering::Release);
    Ok(())
}

/// Suspend all devices
///
/// Ported from: dpm_suspend()
pub fn dpm_suspend(state: PmMessage) -> Result<(), i32> {
    *PM_TRANSITION.lock() = Some(state);
    
    let devices = {
        let list = DPM_LIST.lock();
        list.clone()
    };
    
    let mut suspended = DPM_SUSPENDED_LIST.lock();
    
    for dev in devices.iter().rev() {
        device_suspend(dev, state)?;
        suspended.push(dev.clone());
    }
    
    Ok(())
}

/// Resume all devices
///
/// Ported from: dpm_resume()
pub fn dpm_resume(state: PmMessage) -> Result<(), i32> {
    *PM_TRANSITION.lock() = Some(state);
    
    let devices = {
        let mut suspended = DPM_SUSPENDED_LIST.lock();
        let devs = suspended.clone();
        suspended.clear();
        devs
    };
    
    let mut prepared = DPM_PREPARED_LIST.lock();
    
    for dev in devices {
        device_resume(&dev, state)?;
        prepared.push(dev);
    }
    
    Ok(())
}

/// Prepare device for suspend
///
/// Ported from: device_prepare()
fn device_prepare(dev: &Device, state: PmMessage) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(prepare) = ops.prepare {
            prepare(dev)?;
        }
    }
    
    dev.power.is_prepared.store(true, Ordering::Release);
    Ok(())
}

/// Complete device resume
///
/// Ported from: device_complete()
fn device_complete(dev: &Device, state: PmMessage) {
    if let Some(ops) = &dev.pm_ops {
        if let Some(complete) = ops.complete {
            complete(dev);
        }
    }
    
    dev.power.is_prepared.store(false, Ordering::Release);
}

/// Prepare all devices for suspend
///
/// Ported from: dpm_prepare()
pub fn dpm_prepare(state: PmMessage) -> Result<(), i32> {
    let devices = {
        let list = DPM_LIST.lock();
        list.clone()
    };
    
    let mut prepared = DPM_PREPARED_LIST.lock();
    
    for dev in devices {
        device_prepare(&dev, state)?;
        prepared.push(dev);
    }
    
    Ok(())
}

/// Complete resume for all devices
///
/// Ported from: dpm_complete()
pub fn dpm_complete(state: PmMessage) {
    let devices = {
        let mut prepared = DPM_PREPARED_LIST.lock();
        let devs = prepared.clone();
        prepared.clear();
        devs
    };
    
    for dev in devices {
        device_complete(&dev, state);
    }
}

/// Suspend system
pub fn dpm_suspend_start(state: PmMessage) -> Result<(), i32> {
    dpm_prepare(state)?;
    dpm_suspend(state)
}

/// Resume system
pub fn dpm_resume_end(state: PmMessage) -> Result<(), i32> {
    dpm_resume(state)?;
    dpm_complete(state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pm_event_verb() {
        assert_eq!(PmEvent::Suspend.verb(), "suspend");
        assert_eq!(PmEvent::Resume.verb(), "resume");
        assert_eq!(PmEvent::Hibernate.verb(), "hibernate");
    }

    #[test]
    fn test_device_pm_info_new() {
        let info = DevicePmInfo::new();
        assert!(!info.is_prepared.load(Ordering::Acquire));
        assert!(!info.is_suspended.load(Ordering::Acquire));
        assert!(!info.in_dpm_list.load(Ordering::Acquire));
    }

    #[test]
    fn test_pm_hibernate_is_recovering() {
        assert!(!pm_hibernate_is_recovering());
        
        *PM_TRANSITION.lock() = Some(PmMessage { event: PmEvent::Recover });
        assert!(pm_hibernate_is_recovering());
        
        *PM_TRANSITION.lock() = None;
    }

    #[test]
    fn test_device_pm_add_remove() {
        let dev = Device::mock();
        
        device_pm_add(dev.clone());
        assert!(dev.power.in_dpm_list.load(Ordering::Acquire));
        
        device_pm_remove(&dev);
        assert!(!dev.power.in_dpm_list.load(Ordering::Acquire));
    }

    #[test]
    fn test_dpm_suspend_resume() {
        let dev = Device::mock();
        device_pm_add(dev.clone());
        
        let state = PmMessage { event: PmEvent::Suspend };
        
        let result = dpm_suspend(state);
        assert!(result.is_ok());
        assert!(dev.power.is_suspended.load(Ordering::Acquire));
        
        let resume_state = PmMessage { event: PmEvent::Resume };
        let result = dpm_resume(resume_state);
        assert!(result.is_ok());
        assert!(!dev.power.is_suspended.load(Ordering::Acquire));
    }
}
