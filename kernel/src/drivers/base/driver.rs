// SPDX-License-Identifier: GPL-2.0
/*
 * driver.c - centralized device driver management
 *
 * Ported from Linux drivers/base/driver.c
 * Copyright (c) 2002-3 Patrick Mochel
 * Copyright (c) 2002-3 Open Source Development Labs
 * Copyright (c) 2007 Greg Kroah-Hartman
 */

use super::bus::{BusType, bus_add_driver, bus_remove_driver, bus_is_registered};
use crate::prelude::*;
use super::device::DeviceCore;

/// Device driver core (extended from bus.rs DeviceDriver)
pub struct DriverCore {
    pub name: &'static str,
    pub bus: Option<&'static BusType>,
    pub probe: Option<fn(&DeviceCore) -> DriverResult<()>>,
    pub remove: Option<fn(&DeviceCore) -> DriverResult<()>>,
    pub shutdown: Option<fn(&DeviceCore)>,
    devices: Vec<*mut DeviceCore>,
}

/// Driver errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    InvalidDriver,
    InvalidBus,
    AlreadyRegistered,
    NotRegistered,
    ProbeFailed,
}

pub type DriverResult<T> = core::result::Result<T, DriverError>;

impl DriverCore {
    /// Create a new driver
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            bus: None,
            probe: None,
            remove: None,
            shutdown: None,
            devices: Vec::new(),
        }
    }

    /// Get driver name
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Add device to driver's list
    pub(crate) fn add_device(&mut self, dev: *mut DeviceCore) {
        self.devices.push(dev);
    }

    /// Remove device from driver's list
    pub(crate) fn remove_device(&mut self, dev: *mut DeviceCore) {
        self.devices.retain(|&d| d != dev);
    }

    /// Get all devices bound to this driver
    pub fn devices(&self) -> &[*mut DeviceCore] {
        &self.devices
    }

    /// Get number of bound devices
    pub fn num_devices(&self) -> usize {
        self.devices.len()
    }
}

/// driver_register - register driver with bus
/// @drv: driver to register
///
/// We pass off most of the work to the bus_add_driver() call,
/// since most of the things we have to do deal with the bus
/// structures.
pub fn driver_register(drv: *mut DriverCore) -> DriverResult<()> {
    if drv.is_null() {
        return Err(DriverError::InvalidDriver);
    }

    unsafe {
        // Reject drivers without a probe function
        if (*drv).probe.is_none() {
            return Err(DriverError::InvalidDriver);
        }

        let bus = (*drv).bus.ok_or(DriverError::InvalidBus)?;

        // Check if bus is registered (critical safety check!)
        if !bus_is_registered(bus) {
            return Err(DriverError::InvalidBus);
        }

        // Convert DriverCore to bus DeviceDriver and register
        let bus_drv = &mut *(drv as *mut _ as *mut super::bus::DeviceDriver);
        bus_add_driver(bus_drv).map_err(|_| DriverError::AlreadyRegistered)?;
    }

    Ok(())
}

/// driver_unregister - remove driver from system.
/// @drv: driver.
///
/// Again, we pass off most of the work to the bus-level call.
pub fn driver_unregister(drv: *mut DriverCore) -> DriverResult<()> {
    if drv.is_null() {
        return Err(DriverError::InvalidDriver);
    }

    unsafe {
        let _bus = (*drv).bus.ok_or(DriverError::InvalidBus)?;

        // Remove from bus (CRITICAL - prevents memory corruption!)
        let bus_drv = &mut *(drv as *mut _ as *mut super::bus::DeviceDriver);
        bus_remove_driver(bus_drv).map_err(|_| DriverError::NotRegistered)?;
    }

    Ok(())
}

/// driver_for_each_device - Iterator for devices bound to a driver.
/// @drv: Driver we're iterating.
/// @fn: Function to call for each device.
///
/// Iterate over the @drv's list of devices calling @fn for each one.
pub fn driver_for_each_device<F>(drv: *mut DriverCore, mut f: F) -> DriverResult<()>
where
    F: FnMut(&DeviceCore) -> DriverResult<()>,
{
    if drv.is_null() {
        return Err(DriverError::InvalidDriver);
    }

    // Copy device list to avoid holding lock during callback
    let devices: Vec<*mut DeviceCore> = unsafe {
        (*drv).devices.clone()
    };

    for &dev in &devices {
        unsafe {
            f(&*dev)?;
        }
    }

    Ok(())
}

/// driver_find_device - device iterator for locating a particular device.
/// @drv: The device's driver
/// @match: Callback function to check device
///
/// This is similar to the driver_for_each_device() function above, but
/// it returns a reference to a device that is 'found' for later use, as
/// determined by the @match callback.
pub fn driver_find_device<F>(drv: *mut DriverCore, mut match_fn: F) -> Option<*mut DeviceCore>
where
    F: FnMut(&DeviceCore) -> bool,
{
    if drv.is_null() {
        return None;
    }

    let devices: Vec<*mut DeviceCore> = unsafe {
        (*drv).devices.clone()
    };

    for &dev in &devices {
        unsafe {
            if match_fn(&*dev) {
                return Some(dev);
            }
        }
    }

    None
}

/// driver_attach - try to bind driver to devices
/// @drv: driver
///
/// Walk the list of devices that the bus has and try to
/// match the driver with each one.
pub fn driver_attach(drv: *mut DriverCore) -> DriverResult<()> {
    if drv.is_null() {
        return Err(DriverError::InvalidDriver);
    }

    unsafe {
        let _bus = (*drv).bus.ok_or(DriverError::InvalidBus)?;

        // Iterate all devices on the bus and try to bind
        let bus_drv = &mut *(drv as *mut _ as *mut super::bus::DeviceDriver);
        super::bus::driver_attach(bus_drv).map_err(|_| DriverError::ProbeFailed)?;
    }

    Ok(())
}

/// driver_detach - detach driver from all devices
/// @drv: driver
pub fn driver_detach(drv: *mut DriverCore) -> DriverResult<()> {
    if drv.is_null() {
        return Err(DriverError::InvalidDriver);
    }

    unsafe {
        // Call remove for all bound devices
        let devices = (*drv).devices.clone();
        for &dev in &devices {
            if let Some(remove) = (*drv).remove {
                let _ = remove(&*dev);
            }
        }
        (*drv).devices.clear();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_probe(_dev: &DeviceCore) -> DriverResult<()> {
        Ok(())
    }

    fn test_remove(_dev: &DeviceCore) -> DriverResult<()> {
        Ok(())
    }

    #[test]
    fn test_driver_create() {
        let drv = DriverCore::new("test-driver");
        assert_eq!(drv.name(), "test-driver");
        assert_eq!(drv.num_devices(), 0);
    }

    #[test]
    fn test_driver_devices() {
        use super::super::device::DeviceCore;
        use Box;

        let mut drv = DriverCore::new("test-driver");
        drv.probe = Some(test_probe);
        drv.remove = Some(test_remove);

        let mut dev1 = Box::new(DeviceCore::new("dev1".into()));
        let mut dev2 = Box::new(DeviceCore::new("dev2".into()));

        let dev1_ptr = &mut *dev1 as *mut DeviceCore;
        let dev2_ptr = &mut *dev2 as *mut DeviceCore;

        drv.add_device(dev1_ptr);
        drv.add_device(dev2_ptr);

        assert_eq!(drv.num_devices(), 2);

        drv.remove_device(dev1_ptr);
        assert_eq!(drv.num_devices(), 1);

        core::mem::forget(dev1);
        core::mem::forget(dev2);
    }

    #[test]
    fn test_driver_for_each_device() {
        use super::super::device::DeviceCore;
        use Box;

        let mut drv = DriverCore::new("test-driver");
        let mut dev = Box::new(DeviceCore::new("test-dev".into()));
        let dev_ptr = &mut *dev as *mut DeviceCore;

        drv.add_device(dev_ptr);

        let mut count = 0;
        let result = driver_for_each_device(&mut drv as *mut DriverCore, |_dev| {
            count += 1;
            Ok(())
        });

        assert!(result.is_ok());
        assert_eq!(count, 1);

        core::mem::forget(dev);
    }

    #[test]
    fn test_driver_find_device() {
        use super::super::device::DeviceCore;
        use Box;

        let mut drv = DriverCore::new("test-driver");
        let mut dev = Box::new(DeviceCore::new("target".into()));
        let dev_ptr = &mut *dev as *mut DeviceCore;

        drv.add_device(dev_ptr);

        let found = driver_find_device(&mut drv as *mut DriverCore, |d| {
            d.name() == "target"
        });

        assert!(found.is_some());
        assert_eq!(found.unwrap(), dev_ptr);

        core::mem::forget(dev);
    }
}
