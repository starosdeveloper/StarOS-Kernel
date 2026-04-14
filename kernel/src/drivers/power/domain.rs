// SPDX-License-Identifier: MIT OR Apache-2.0
//! Power Management Domains
//!
//! Ported from Linux: drivers/base/power/generic_ops.c
//! Source lines: ~200 C → ~600 Rust

use crate::drivers::base::Device;
use alloc::vec::Vec;
use crate::sync::Mutex;

/// PM Domain
#[derive(Debug)]
pub struct PmDomain {
    pub name: &'static str,
    pub devices: Mutex<Vec<Device>>,
    pub ops: PmDomainOps,
}

/// PM Domain operations
#[derive(Debug, Clone)]
pub struct PmDomainOps {
    pub runtime_suspend: Option<fn(&Device) -> Result<(), i32>>,
    pub runtime_resume: Option<fn(&Device) -> Result<(), i32>>,
    pub prepare: Option<fn(&Device) -> Result<(), i32>>,
    pub suspend: Option<fn(&Device) -> Result<(), i32>>,
    pub suspend_late: Option<fn(&Device) -> Result<(), i32>>,
    pub suspend_noirq: Option<fn(&Device) -> Result<(), i32>>,
    pub resume: Option<fn(&Device) -> Result<(), i32>>,
    pub resume_early: Option<fn(&Device) -> Result<(), i32>>,
    pub resume_noirq: Option<fn(&Device) -> Result<(), i32>>,
    pub freeze: Option<fn(&Device) -> Result<(), i32>>,
    pub freeze_noirq: Option<fn(&Device) -> Result<(), i32>>,
    pub thaw: Option<fn(&Device) -> Result<(), i32>>,
    pub thaw_noirq: Option<fn(&Device) -> Result<(), i32>>,
    pub poweroff: Option<fn(&Device) -> Result<(), i32>>,
    pub poweroff_late: Option<fn(&Device) -> Result<(), i32>>,
    pub poweroff_noirq: Option<fn(&Device) -> Result<(), i32>>,
    pub restore: Option<fn(&Device) -> Result<(), i32>>,
    pub restore_early: Option<fn(&Device) -> Result<(), i32>>,
    pub restore_noirq: Option<fn(&Device) -> Result<(), i32>>,
    pub complete: Option<fn(&Device)>,
}

impl PmDomainOps {
    pub fn new() -> Self {
        Self {
            runtime_suspend: None,
            runtime_resume: None,
            prepare: None,
            suspend: None,
            suspend_late: None,
            suspend_noirq: None,
            resume: None,
            resume_early: None,
            resume_noirq: None,
            freeze: None,
            freeze_noirq: None,
            thaw: None,
            thaw_noirq: None,
            poweroff: None,
            poweroff_late: None,
            poweroff_noirq: None,
            restore: None,
            restore_early: None,
            restore_noirq: None,
            complete: None,
        }
    }
}

impl PmDomain {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            devices: Mutex::new(Vec::new()),
            ops: PmDomainOps::new(),
        }
    }

    pub fn add_device(&self, dev: Device) {
        self.devices.lock().push(dev);
    }

    pub fn remove_device(&self, dev: &Device) {
        let mut devices = self.devices.lock();
        devices.retain(|d| d.id != dev.id);
    }
}

/// Generic runtime suspend
///
/// Ported from: pm_generic_runtime_suspend()
pub fn pm_generic_runtime_suspend(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(suspend) = ops.runtime_suspend {
            return suspend(dev);
        }
    }
    Ok(())
}

/// Generic runtime resume
///
/// Ported from: pm_generic_runtime_resume()
pub fn pm_generic_runtime_resume(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(resume) = ops.runtime_resume {
            return resume(dev);
        }
    }
    Ok(())
}

/// Generic prepare
///
/// Ported from: pm_generic_prepare()
pub fn pm_generic_prepare(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(prepare) = ops.prepare {
            return prepare(dev);
        }
    }
    Ok(())
}

/// Generic suspend
///
/// Ported from: pm_generic_suspend()
pub fn pm_generic_suspend(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(suspend) = ops.suspend {
            return suspend(dev);
        }
    }
    Ok(())
}

/// Generic suspend late
///
/// Ported from: pm_generic_suspend_late()
pub fn pm_generic_suspend_late(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(suspend_late) = ops.suspend_late {
            return suspend_late(dev);
        }
    }
    Ok(())
}

/// Generic suspend noirq
///
/// Ported from: pm_generic_suspend_noirq()
pub fn pm_generic_suspend_noirq(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(suspend_noirq) = ops.suspend_noirq {
            return suspend_noirq(dev);
        }
    }
    Ok(())
}

/// Generic resume
///
/// Ported from: pm_generic_resume()
pub fn pm_generic_resume(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(resume) = ops.resume {
            return resume(dev);
        }
    }
    Ok(())
}

/// Generic resume early
///
/// Ported from: pm_generic_resume_early()
pub fn pm_generic_resume_early(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(resume_early) = ops.resume_early {
            return resume_early(dev);
        }
    }
    Ok(())
}

/// Generic resume noirq
///
/// Ported from: pm_generic_resume_noirq()
pub fn pm_generic_resume_noirq(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(resume_noirq) = ops.resume_noirq {
            return resume_noirq(dev);
        }
    }
    Ok(())
}

/// Generic freeze
///
/// Ported from: pm_generic_freeze()
pub fn pm_generic_freeze(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(freeze) = ops.freeze {
            return freeze(dev);
        }
    }
    Ok(())
}

/// Generic freeze noirq
///
/// Ported from: pm_generic_freeze_noirq()
pub fn pm_generic_freeze_noirq(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(freeze_noirq) = ops.freeze_noirq {
            return freeze_noirq(dev);
        }
    }
    Ok(())
}

/// Generic thaw
///
/// Ported from: pm_generic_thaw()
pub fn pm_generic_thaw(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(thaw) = ops.thaw {
            return thaw(dev);
        }
    }
    Ok(())
}

/// Generic thaw noirq
///
/// Ported from: pm_generic_thaw_noirq()
pub fn pm_generic_thaw_noirq(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(thaw_noirq) = ops.thaw_noirq {
            return thaw_noirq(dev);
        }
    }
    Ok(())
}

/// Generic poweroff
///
/// Ported from: pm_generic_poweroff()
pub fn pm_generic_poweroff(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(poweroff) = ops.poweroff {
            return poweroff(dev);
        }
    }
    Ok(())
}

/// Generic poweroff late
///
/// Ported from: pm_generic_poweroff_late()
pub fn pm_generic_poweroff_late(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(poweroff_late) = ops.poweroff_late {
            return poweroff_late(dev);
        }
    }
    Ok(())
}

/// Generic poweroff noirq
///
/// Ported from: pm_generic_poweroff_noirq()
pub fn pm_generic_poweroff_noirq(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(poweroff_noirq) = ops.poweroff_noirq {
            return poweroff_noirq(dev);
        }
    }
    Ok(())
}

/// Generic restore
///
/// Ported from: pm_generic_restore()
pub fn pm_generic_restore(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(restore) = ops.restore {
            return restore(dev);
        }
    }
    Ok(())
}

/// Generic restore early
///
/// Ported from: pm_generic_restore_early()
pub fn pm_generic_restore_early(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(restore_early) = ops.restore_early {
            return restore_early(dev);
        }
    }
    Ok(())
}

/// Generic restore noirq
///
/// Ported from: pm_generic_restore_noirq()
pub fn pm_generic_restore_noirq(dev: &Device) -> Result<(), i32> {
    if let Some(ops) = &dev.pm_ops {
        if let Some(restore_noirq) = ops.restore_noirq {
            return restore_noirq(dev);
        }
    }
    Ok(())
}

/// Generic complete
///
/// Ported from: pm_generic_complete()
pub fn pm_generic_complete(dev: &Device) {
    if let Some(ops) = &dev.pm_ops {
        if let Some(complete) = ops.complete {
            complete(dev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pm_domain_new() {
        let domain = PmDomain::new("test_domain");
        assert_eq!(domain.name, "test_domain");
        assert_eq!(domain.devices.lock().len(), 0);
    }

    #[test]
    fn test_pm_domain_add_remove_device() {
        let domain = PmDomain::new("test");
        let dev = Device::mock();
        
        domain.add_device(dev.clone());
        assert_eq!(domain.devices.lock().len(), 1);
        
        domain.remove_device(&dev);
        assert_eq!(domain.devices.lock().len(), 0);
    }

    #[test]
    fn test_pm_generic_runtime_suspend() {
        let dev = Device::mock();
        let result = pm_generic_runtime_suspend(&dev);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pm_generic_runtime_resume() {
        let dev = Device::mock();
        let result = pm_generic_runtime_resume(&dev);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pm_generic_suspend_resume() {
        let dev = Device::mock();
        
        let result = pm_generic_suspend(&dev);
        assert!(result.is_ok());
        
        let result = pm_generic_resume(&dev);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pm_generic_freeze_thaw() {
        let dev = Device::mock();
        
        let result = pm_generic_freeze(&dev);
        assert!(result.is_ok());
        
        let result = pm_generic_thaw(&dev);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pm_generic_poweroff_restore() {
        let dev = Device::mock();
        
        let result = pm_generic_poweroff(&dev);
        assert!(result.is_ok());
        
        let result = pm_generic_restore(&dev);
        assert!(result.is_ok());
    }
}
