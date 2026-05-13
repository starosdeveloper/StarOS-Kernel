// SPDX-License-Identifier: GPL-2.0
/*
 * bus.c - bus driver management
 *
 * Ported from Linux drivers/base/bus.c
 * Copyright (c) 2002-3 Patrick Mochel
 * Copyright (c) 2002-3 Open Source Development Labs
 * Copyright (c) 2007 Greg Kroah-Hartman
 */

use crate::prelude::*;
use spin::Mutex;

/// Bus type descriptor
#[derive(Debug)]
pub struct BusType {
    pub name: &'static str,
    pub match_fn: Option<fn(&Device, &DeviceDriver) -> bool>,
    pub probe_fn: Option<fn(&Device) -> core::result::Result<(), BusError>>,
    pub remove_fn: Option<fn(&Device) -> core::result::Result<(), BusError>>,
}

/// Device descriptor
#[derive(Debug)]
pub struct Device {
    pub name: String,
    pub bus: Option<&'static BusType>,
    pub driver: Option<SendPtr<DeviceDriver>>,
    priv_data: Option<usize>,
    pub dma_mask: core::sync::atomic::AtomicU64,
    pub coherent_dma_mask: core::sync::atomic::AtomicU64,
}

/// Device driver descriptor
#[derive(Debug)]
pub struct DeviceDriver {
    pub name: &'static str,
    pub bus: Option<&'static BusType>,
    pub probe: Option<fn(&Device) -> core::result::Result<(), BusError>>,
    pub remove: Option<fn(&Device) -> core::result::Result<(), BusError>>,
    devices: Vec<*mut Device>,
}

/// Send-safe wrapper for raw pointers
#[derive(Debug)]
pub struct SendPtr<T> {
    ptr: *mut T,
}

// SAFETY: SendPtr is used only for device/driver pointers in bus registry
// All access is synchronized through BUS_REGISTRY Mutex
// Pointers remain valid for lifetime of bus subsystem
unsafe impl<T> Send for SendPtr<T> {}
// SAFETY: All mutations go through Mutex-protected registry
unsafe impl<T> Sync for SendPtr<T> {}

impl<T> SendPtr<T> {
    pub fn new(ptr: *mut T) -> Self {
        Self { ptr }
    }
    
    pub fn as_ptr(&self) -> *mut T {
        self.ptr
    }
    
    pub fn get(&self) -> *mut T {
        self.ptr
    }
}

impl<T> Copy for SendPtr<T> {}

impl<T> Clone for SendPtr<T> {
    fn clone(&self) -> Self {
        Self { ptr: self.ptr }
    }
}

impl<T> PartialEq for SendPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl<T> Eq for SendPtr<T> {}

/// Bus subsystem private data
struct SubsysPrivate {
    bus: &'static BusType,
    devices: Vec<SendPtr<Device>>,
    drivers: Vec<SendPtr<DeviceDriver>>,
    drivers_autoprobe: bool,
    /// Reference count for this subsystem
    refcount: usize,
}

/// Global bus registry
static BUS_REGISTRY: Mutex<Vec<SubsysPrivate>> = Mutex::new(Vec::new());

/// Bus errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusError {
    InvalidBus,
    InvalidDevice,
    InvalidDriver,
    NoMatch,
    Busy,
    NoMemory,
}

pub type Result<T> = core::result::Result<T, BusError>;

impl SubsysPrivate {
    /// Create new subsystem private data
    fn new(bus: &'static BusType) -> Self {
        Self {
            bus,
            devices: Vec::new(),
            drivers: Vec::new(),
            drivers_autoprobe: true,
            refcount: 1, // Start with refcount of 1
        }
    }

    /// Increment reference count
    fn get(&mut self) {
        self.refcount += 1;
    }

    /// Decrement reference count, returns true if should be freed
    fn put(&mut self) -> bool {
        if self.refcount > 0 {
            self.refcount -= 1;
        }
        self.refcount == 0
    }
}

/// bus_to_subsys - Turn a struct bus_type into a struct subsys_private
/// @bus: pointer to the struct bus_type to look up
///
/// The driver core internals needs to work on the subsys_private structure, not
/// the external struct bus_type pointer. This function walks the list of
/// registered busses in the system and finds the matching one and returns the
/// internal struct subsys_private that relates to that bus.
///
/// Note, the reference count of the return value is INCREMENTED if it is not
/// NULL. A call to subsys_put() must be done when finished with the pointer in
/// order for it to be properly freed.
///
/// Ported from Linux drivers/base/bus.c::bus_to_subsys()
fn bus_to_subsys(bus: &'static BusType) -> Option<usize> {
    let mut registry = BUS_REGISTRY.lock();
    
    // Find the subsys_private for this bus
    for (idx, sp) in registry.iter_mut().enumerate() {
        if sp.bus.name == bus.name {
            // Increment reference count
            sp.get();
            return Some(idx);
        }
    }
    
    None
}

/// subsys_put - decrement reference count on subsys_private
/// @sp_idx: index of subsys_private in registry
///
/// Decrements the reference count. When it reaches zero, the subsystem
/// can be freed (though we don't actually free it in this implementation).
///
/// Ported from Linux drivers/base/base.h::subsys_put()
fn subsys_put(sp_idx: usize) {
    let mut registry = BUS_REGISTRY.lock();
    if let Some(sp) = registry.get_mut(sp_idx) {
        let should_free = sp.put();
        // In full Linux implementation, this would trigger cleanup
        // For now, we just decrement the refcount
        if should_free {
            // Could remove from registry here, but keeping it simple
            // as buses are typically long-lived
        }
    }
}

impl Device {
    /// Create a new device
    pub fn new(name: String) -> Self {
        Self {
            name,
            bus: None,
            driver: None,
            priv_data: None,
            dma_mask: core::sync::atomic::AtomicU64::new(0),
            coherent_dma_mask: core::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Set private data
    pub fn set_priv_data(&mut self, data: usize) {
        self.priv_data = Some(data);
    }

    /// Get private data
    pub fn get_priv_data(&self) -> Option<usize> {
        self.priv_data
    }

    /// Check if device has IOMMU
    pub fn has_iommu(&self) -> bool {
        false // TODO: Implement IOMMU detection
    }

    /// Check if device is DMA coherent
    pub fn is_dma_coherent(&self) -> bool {
        false // TODO: Implement DMA coherency detection
    }

    /// Add resource to device
    pub fn add_resource(&self, _resource: alloc::boxed::Box<dyn core::any::Any>) {
        // TODO: Implement resource management
    }

    /// Remove resource from device
    pub fn remove_resource<F>(&self, _predicate: F)
    where
        F: FnMut(&dyn core::any::Any) -> bool,
    {
        // TODO: Implement resource management
    }
}

impl DeviceDriver {
    /// Create a new driver
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            bus: None,
            probe: None,
            remove: None,
            devices: Vec::new(),
        }
    }
}

/// bus_register - register a driver-core subsystem
/// @bus: bus to register
///
/// Once we have that, we register the bus with the kobject
/// infrastructure, then register the children subsystems it has:
/// the devices and drivers that belong to the subsystem.
pub fn bus_register(bus: &'static BusType) -> Result<()> {
    let mut registry = BUS_REGISTRY.lock();

    // Check if already registered
    if registry.iter().any(|sp| sp.bus.name == bus.name) {
        return Err(BusError::Busy);
    }

    // Create subsys_private with refcount = 1
    let sp = SubsysPrivate::new(bus);
    registry.push(sp);
    Ok(())
}

/// bus_unregister - remove a bus from the system
/// @bus: bus.
///
/// Unregister the child subsystems and the bus itself.
pub fn bus_unregister(bus: &'static BusType) -> Result<()> {
    let mut registry = BUS_REGISTRY.lock();

    let pos = registry
        .iter()
        .position(|sp| sp.bus.name == bus.name)
        .ok_or(BusError::InvalidBus)?;

    registry.remove(pos);
    Ok(())
}

/// bus_add_device - add device to bus
/// @dev: device being added
///
/// - Add the device to its bus's list of devices.
pub fn bus_add_device(dev: *mut Device) -> Result<()> {
    // SAFETY: Caller must ensure dev is valid pointer to Device
    // Device must remain valid for lifetime of bus registration
    // Access to Device fields is synchronized through BUS_REGISTRY lock
    unsafe {
        let bus = (*dev).bus.ok_or(BusError::InvalidBus)?;
        let mut registry = BUS_REGISTRY.lock();

        let sp = registry
            .iter_mut()
            .find(|sp| sp.bus.name == bus.name)
            .ok_or(BusError::InvalidBus)?;

        // Prevent double-registration which could corrupt internal lists
        if sp.devices.iter().any(|d| d.get() == dev) {
            return Err(BusError::Busy);
        }

        sp.devices.push(SendPtr::new(dev));
        Ok(())
    }
}

/// bus_remove_device - remove device from bus
/// @dev: device to be removed
///
/// - Delete device from bus's list.
/// - Detach from its driver.
pub fn bus_remove_device(dev: *mut Device) -> Result<()> {
    // SAFETY: Caller must ensure dev is valid pointer to Device
    // Device must have been previously added via bus_add_device
    // Access synchronized through BUS_REGISTRY lock
    unsafe {
        let bus = (*dev).bus.ok_or(BusError::InvalidBus)?;
        let mut registry = BUS_REGISTRY.lock();

        let sp = registry
            .iter_mut()
            .find(|sp| sp.bus.name == bus.name)
            .ok_or(BusError::InvalidBus)?;

        sp.devices.retain(|&d| d.get() != dev);

        // Detach driver
        // SAFETY: drv pointer is valid if Some, from previous driver_attach
        if let Some(drv) = (*dev).driver {
            if let Some(remove) = (*drv.get()).remove {
                // SAFETY: dev is valid, dereferencing for callback
                let _ = remove(&*dev);
            }
            (*dev).driver = None;
        }

        Ok(())
    }
}

/// bus_add_driver - Add a driver to the bus.
/// @drv: driver.
pub fn bus_add_driver(drv: *mut DeviceDriver) -> Result<()> {
    // SAFETY: Caller must ensure drv is valid pointer to DeviceDriver
    // Driver must remain valid for lifetime of bus registration
    // Access synchronized through BUS_REGISTRY lock
    unsafe {
        let bus = (*drv).bus.ok_or(BusError::InvalidBus)?;
        let mut registry = BUS_REGISTRY.lock();

        let sp = registry
            .iter_mut()
            .find(|sp| sp.bus.name == bus.name)
            .ok_or(BusError::InvalidBus)?;

        sp.drivers.push(SendPtr::new(drv));

        // Auto-probe if enabled
        if sp.drivers_autoprobe {
            drop(registry); // Release lock before calling driver_attach
            driver_attach(drv)?;
        }

        Ok(())
    }
}

/// bus_remove_driver - delete driver from bus's knowledge.
/// @drv: driver.
///
/// Detach the driver from the devices it controls, and remove
/// it from its bus's list of drivers.
pub fn bus_remove_driver(drv: *mut DeviceDriver) -> Result<()> {
    // SAFETY: Caller must ensure drv is valid pointer to DeviceDriver
    // Driver must have been previously added via bus_add_driver
    // Access synchronized through BUS_REGISTRY lock
    unsafe {
        let bus = (*drv).bus.ok_or(BusError::InvalidBus)?;
        let mut registry = BUS_REGISTRY.lock();

        let sp = registry
            .iter_mut()
            .find(|sp| sp.bus.name == bus.name)
            .ok_or(BusError::InvalidBus)?;

        sp.drivers.retain(|&d| d.get() != drv);

        // Detach from all devices
        for &dev in &sp.devices {
            let dev_ptr = dev.get();
            // SAFETY: dev_ptr is valid, from devices list
            if (*dev_ptr).driver == Some(SendPtr::new(drv)) {
                // SAFETY: drv is valid, remove callback is valid function pointer
                if let Some(remove) = (*drv).remove {
                    let _ = remove(&*dev_ptr);
                }
                (*dev_ptr).driver = None;
            }
        }

        Ok(())
    }
}

/// bus_for_each_dev - device iterator.
/// @bus: bus type.
/// @data: data for the callback.
/// @fn: function to be called for each device.
///
/// Iterate over @bus's list of devices, and call @fn for each,
/// passing it @data.
pub fn bus_for_each_dev<F>(bus: &'static BusType, mut f: F) -> Result<()>
where
    F: FnMut(&Device) -> Result<()>,
{
    let devices: Vec<SendPtr<Device>> = {
        let registry = BUS_REGISTRY.lock();
        let sp = registry
            .iter()
            .find(|sp| sp.bus.name == bus.name)
            .ok_or(BusError::InvalidBus)?;
        sp.devices.clone()
    };

    for &dev in &devices {
        let dev_ptr: *mut Device = dev.get();
        // SAFETY: dev_ptr is valid, from devices list, remains valid during iteration
        unsafe {
            f(&*dev_ptr)?;
        }
    }

    Ok(())
}

/// bus_is_registered - check if a bus is registered
/// @bus: bus to check
///
/// Returns true if the bus is registered in the system, false otherwise.
/// This function uses bus_to_subsys() which increments the reference count,
/// so we must call subsys_put() to decrement it.
///
/// Ported from Linux drivers/base/bus.c::bus_is_registered()
pub fn bus_is_registered(bus: &'static BusType) -> bool {
    let sp_idx = bus_to_subsys(bus);
    let is_initialized = sp_idx.is_some();
    
    if let Some(idx) = sp_idx {
        // Must decrement reference count
        subsys_put(idx);
    }
    
    is_initialized
}

/// bus_for_each_drv - driver iterator
/// @bus: the bus we're dealing with.
/// @fn: function to call for each driver.
///
/// Iterate over each driver that belongs to @bus, and call @fn for each.
pub fn bus_for_each_drv<F>(bus: &'static BusType, mut f: F) -> Result<()>
where
    F: FnMut(&DeviceDriver) -> Result<()>,
{
    let drivers: Vec<SendPtr<DeviceDriver>> = {
        let registry = BUS_REGISTRY.lock();
        let sp = registry
            .iter()
            .find(|sp| sp.bus.name == bus.name)
            .ok_or(BusError::InvalidBus)?;
        sp.drivers.clone()
    };

    for &drv in &drivers {
        let drv_ptr: *mut DeviceDriver = drv.get();
        // SAFETY: drv_ptr is valid, from drivers list, remains valid during iteration
        unsafe {
            f(&*drv_ptr)?;
        }
    }

    Ok(())
}

/// driver_attach - try to bind driver to devices.
/// @drv: driver.
///
/// Walk the list of devices that the bus has on it and try to
/// match the driver with each one.  If driver_probe_device()
/// returns 0 and the @dev->driver is set, we've found a
/// compatible pair.
pub(crate) fn driver_attach(drv: *mut DeviceDriver) -> Result<()> {
    // SAFETY: Caller must ensure drv is valid pointer to DeviceDriver
    // Driver must be registered on a bus
    unsafe {
        let bus = (*drv).bus.ok_or(BusError::InvalidBus)?;

        let devices: Vec<SendPtr<Device>> = {
            let registry = BUS_REGISTRY.lock();
            let sp = registry
                .iter()
                .find(|sp| sp.bus.name == bus.name)
                .ok_or(BusError::InvalidBus)?;
            sp.devices.clone()
        };

        for &dev in &devices {
            let dev_ptr: *mut Device = dev.get();
            // SAFETY: dev_ptr is valid, from devices list
            if (*dev_ptr).driver.is_none() {
                let _ = driver_probe_device(drv, dev_ptr);
            }
        }

        Ok(())
    }
}

/// driver_probe_device - attempt to bind device & driver together
/// @drv: driver to bind a device to
/// @dev: device to try to bind to the driver
///
/// This function returns -ENODEV if the device is not registered, -EBUSY if it
/// already has a driver, and 0 on success.
fn driver_probe_device(drv: *mut DeviceDriver, dev: *mut Device) -> Result<()> {
    // SAFETY: Caller must ensure drv and dev are valid pointers
    // Both must be registered on the same bus
    unsafe {
        // Check if device already has a driver
        if (*dev).driver.is_some() {
            return Err(BusError::Busy);
        }

        // Check if driver matches device
        let bus = (*dev).bus.ok_or(BusError::InvalidBus)?;
        if let Some(match_fn) = bus.match_fn {
            // SAFETY: dev and drv are valid, dereferencing for callback
            if !match_fn(&*dev, &*drv) {
                return Err(BusError::NoMatch);
            }
        }

        // Call driver's probe function
        if let Some(probe) = (*drv).probe {
            probe(&*dev)?;
        }

        // Bind driver to device
        (*dev).driver = Some(SendPtr::new(drv));
        (*drv).devices.push(dev);

        Ok(())
    }
}

/// device_attach - try to attach device to a driver.
/// @dev: device.
///
/// Walk the list of drivers that the bus has and call
/// driver_probe_device() for each pair. If a compatible
/// pair is found, break out and return.
pub fn device_attach(dev: *mut Device) -> Result<()> {
    unsafe {
        let bus = (*dev).bus.ok_or(BusError::InvalidBus)?;

        let drivers: Vec<SendPtr<DeviceDriver>> = {
            let registry = BUS_REGISTRY.lock();
            let sp = registry
                .iter()
                .find(|sp| sp.bus.name == bus.name)
                .ok_or(BusError::InvalidBus)?;
            sp.drivers.clone()
        };

        for &drv in &drivers {
            let drv_ptr: *mut DeviceDriver = drv.get();
            if driver_probe_device(drv_ptr, dev).is_ok() {
                return Ok(());
            }
        }

        Err(BusError::NoMatch)
    }
}

/// bus_probe_device - probe drivers for a new device
/// @dev: device to probe
///
/// - Automatically probe for a driver if the bus allows it.
pub fn bus_probe_device(dev: *mut Device) -> Result<()> {
    device_attach(dev)
}

/// bus_rescan_devices - rescan devices on the bus for possible drivers
/// @bus: the bus to scan.
///
/// This function will look for devices on the bus with no driver
/// attached and rescan it against existing drivers.
pub fn bus_rescan_devices(bus: &'static BusType) -> Result<()> {
    let devices: Vec<SendPtr<Device>> = {
        let registry = BUS_REGISTRY.lock();
        let sp = registry
            .iter()
            .find(|sp| sp.bus.name == bus.name)
            .ok_or(BusError::InvalidBus)?;
        sp.devices.clone()
    };

    for &dev in &devices {
        let dev_ptr: *mut Device = dev.get();
        unsafe {
            if (*dev_ptr).driver.is_none() {
                let _ = device_attach(dev_ptr);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_BUS: BusType = BusType {
        name: "test-bus",
        match_fn: Some(test_match),
        probe_fn: None,
        remove_fn: None,
    };

    fn test_match(_dev: &Device, _drv: &DeviceDriver) -> bool {
        true
    }

    fn test_probe(_dev: &Device) -> Result<()> {
        Ok(())
    }

    #[test]
    fn test_bus_register() {
        let result = bus_register(&TEST_BUS);
        assert!(result.is_ok());
        bus_unregister(&TEST_BUS).unwrap();
    }

    #[test]
    fn test_bus_is_registered() {
        // Bus should not be registered initially
        assert!(!bus_is_registered(&TEST_BUS));
        
        // Register bus
        bus_register(&TEST_BUS).unwrap();
        
        // Now it should be registered
        assert!(bus_is_registered(&TEST_BUS));
        
        // Unregister
        bus_unregister(&TEST_BUS).unwrap();
        
        // Should not be registered anymore
        assert!(!bus_is_registered(&TEST_BUS));
    }

    #[test]
    fn test_bus_add_device() {
        bus_register(&TEST_BUS).unwrap();
        let mut dev = Device::new("test-device".into());
        dev.bus = Some(&TEST_BUS);
        let result = bus_add_device(&mut dev as *mut Device);
        assert!(result.is_ok());
        bus_remove_device(&mut dev as *mut Device).unwrap();
        bus_unregister(&TEST_BUS).unwrap();
    }

    #[test]
    fn test_bus_add_driver() {
        bus_register(&TEST_BUS).unwrap();
        let mut drv = DeviceDriver::new("test-driver");
        drv.bus = Some(&TEST_BUS);
        drv.probe = Some(test_probe);
        let result = bus_add_driver(&mut drv as *mut DeviceDriver);
        assert!(result.is_ok());
        bus_remove_driver(&mut drv as *mut DeviceDriver).unwrap();
        bus_unregister(&TEST_BUS).unwrap();
    }

    #[test]
    fn test_device_driver_binding() {
        bus_register(&TEST_BUS).unwrap();

        let mut dev = Device::new("test-device".into());
        dev.bus = Some(&TEST_BUS);
        bus_add_device(&mut dev as *mut Device).unwrap();

        let mut drv = DeviceDriver::new("test-driver");
        drv.bus = Some(&TEST_BUS);
        drv.probe = Some(test_probe);
        bus_add_driver(&mut drv as *mut DeviceDriver).unwrap();

        // Driver should auto-probe and bind
        unsafe {
            assert!(dev.driver.is_some());
        }

        bus_remove_driver(&mut drv as *mut DeviceDriver).unwrap();
        bus_remove_device(&mut dev as *mut Device).unwrap();
        bus_unregister(&TEST_BUS).unwrap();
    }
}
