// SPDX-License-Identifier: MIT OR Apache-2.0
//! Power Management Subsystem

pub mod core;
pub mod runtime;
pub mod domain;

pub use core::{
    PmEvent, PmMessage, DevicePmInfo,
    pm_hibernate_is_recovering,
    device_pm_sleep_init, device_pm_lock, device_pm_unlock,
    device_pm_add, device_pm_remove,
    device_pm_move_before, device_pm_move_after, device_pm_move_last,
    dpm_suspend, dpm_resume, dpm_prepare, dpm_complete,
    dpm_suspend_start, dpm_resume_end,
};

pub use runtime::{
    RpmStatus, RuntimePmInfo,
    pm_runtime_enable, pm_runtime_disable,
    pm_runtime_get_noresume, pm_runtime_put_noidle,
    pm_runtime_get_sync, pm_runtime_put, pm_runtime_put_autosuspend,
    pm_runtime_set_autosuspend_delay,
    pm_runtime_use_autosuspend, pm_runtime_dont_use_autosuspend,
    pm_runtime_set_active, pm_runtime_set_suspended,
    pm_runtime_block, pm_runtime_unblock,
    pm_runtime_active_time, pm_runtime_suspended_time,
};

pub use domain::{
    PmDomain, PmDomainOps,
    pm_generic_runtime_suspend, pm_generic_runtime_resume,
    pm_generic_prepare, pm_generic_complete,
    pm_generic_suspend, pm_generic_suspend_late, pm_generic_suspend_noirq,
    pm_generic_resume, pm_generic_resume_early, pm_generic_resume_noirq,
    pm_generic_freeze, pm_generic_freeze_noirq,
    pm_generic_thaw, pm_generic_thaw_noirq,
    pm_generic_poweroff, pm_generic_poweroff_late, pm_generic_poweroff_noirq,
    pm_generic_restore, pm_generic_restore_early, pm_generic_restore_noirq,
};
