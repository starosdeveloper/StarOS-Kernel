// SPDX-License-Identifier: GPL-2.0
/*
 * platform.c - platform 'pseudo' bus for legacy devices
 *
 * Ported from Linux drivers/base/platform.c
 * Copyright (c) 2002-3 Patrick Mochel
 * Copyright (c) 2002-3 Open Source Development Labs
 *
 * Please see Documentation/driver-api/driver-model/platform.rst for more
 * information.
 */

use crate::prelude::*;
use crate::drivers::resource::core::{Resource, IORESOURCE_MEM, IORESOURCE_IO, IORESOURCE_IRQ};
use crate::drivers::resource::mmio::MmioRegion;
use crate::drivers::base::device::DeviceCore;
use crate::drivers::base::driver::DriverCore;
use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::vec::Vec;
use alloc::string::String;
use alloc::boxed::Box;

/// For automatically allocated device IDs
static PLATFORM_DEVID_IDA: AtomicUsize = AtomicUsize::new(0);

/// Platform device ID constants
pub const PLATFORM_DEVID_NONE: i32 = -1;
pub const PLATFORM_DEVID_AUTO: i32 = -2;

/// Platform device structure
pub struct PlatformDevice {
    /// Device name
    pub name: String,
    /// Device ID (-1 for none, -2 for auto)
    pub id: i32,
    /// Whether ID was auto-allocated
    pub id_auto: bool,
    /// Resources (MMIO, IRQ, DMA, etc)
    pub resources: Vec<Resource>,
    /// Number of resources
    pub num_resources: usize,
    /// Device core
    pub dev: DeviceCore,
    /// Platform-specific data
    pub platform_data: Option<*mut u8>,
    /// Driver override
    pub driver_override: Option<String>,
    /// DMA mask
    pub platform_dma_mask: u64,
    /// Device Tree node
    pub of_node: Option<*mut u8>,
}

/// Platform driver structure
pub struct PlatformDriver {
    /// Probe function
    pub probe: Option<fn(&mut PlatformDevice) -> Result<()>>,
    /// Remove function
    pub remove: Option<fn(&mut PlatformDevice)>,
    /// Shutdown function
    pub shutdown: Option<fn(&mut PlatformDevice)>,
    /// Suspend function
    pub suspend: Option<fn(&mut PlatformDevice) -> Result<()>>,
    /// Resume function
    pub resume: Option<fn(&mut PlatformDevice) -> Result<()>>,
    /// Driver core
    pub driver: DriverCore,
    /// ID table for matching
    pub id_table: Option<&'static [PlatformDeviceId]>,
    /// Prevent deferred probe
    pub prevent_deferred_probe: bool,
}

/// Platform device ID for matching
#[derive(Debug, Clone)]
pub struct PlatformDeviceId {
    pub name: &'static str,
    pub driver_data: usize,
}

/// Platform device info for registration
pub struct PlatformDeviceInfo {
    pub parent: Option<*mut DeviceCore>,
    pub fwnode: Option<*mut u8>,
    pub name: String,
    pub id: i32,
    pub res: Option<Vec<Resource>>,
    pub num_res: usize,
    pub data: Option<*const u8>,
    pub size_data: usize,
    pub dma_mask: u64,
    pub properties: Option<*const u8>,
    pub of_node_reused: bool,
}

/// Platform errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformError {
    NoDevice,
    NoMemory,
    Invalid,
    Busy,
    ProbeDefer,
    Range,
    NoSpace,
    Permission,
}

pub type Result<T> = core::result::Result<T, PlatformError>;

impl PlatformDevice {
    /// Create a new platform device
    pub fn new(name: String, id: i32) -> Self {
        Self {
            name: name.clone(),
            id,
            id_auto: false,
            resources: Vec::new(),
            num_resources: 0,
            dev: DeviceCore::new(name),
            platform_data: None,
            driver_override: None,
            platform_dma_mask: 0xFFFFFFFF, // DMA_BIT_MASK(32)
            of_node: None,
        }
    }

    /// Get device name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get device ID
    pub fn id(&self) -> i32 {
        self.id
    }
}

impl PlatformDriver {
    /// Create a new platform driver
    pub fn new(name: &'static str) -> Self {
        Self {
            probe: None,
            remove: None,
            shutdown: None,
            suspend: None,
            resume: None,
            driver: DriverCore::new(name),
            id_table: None,
            prevent_deferred_probe: false,
        }
    }
}

/// Get resource type from flags
#[inline]
fn resource_type(r: &Resource) -> u64 {
    r.flags & (IORESOURCE_IO | IORESOURCE_MEM | IORESOURCE_IRQ)
}

/// platform_get_resource - get a resource for a device
/// @dev: platform device
/// @type: resource type
/// @num: resource index
///
/// Return: a pointer to the resource or None on failure.
pub fn platform_get_resource(
    dev: &PlatformDevice,
    res_type: u64,
    num: usize,
) -> Option<&Resource> {
    if num >= dev.num_resources {
        return None;
    }

    let mut count = num;
    
    for i in 0..dev.num_resources {
        let r = &dev.resources[i];
        
        if res_type == resource_type(r) {
            if count == 0 {
                return Some(r);
            }
            count -= 1;
        }
    }
    
    None
}

/// platform_get_resource (mutable version)
pub fn platform_get_resource_mut(
    dev: &mut PlatformDevice,
    res_type: u64,
    num: usize,
) -> Option<&mut Resource> {
    let mut count = num;
    
    for i in 0..dev.num_resources {
        if res_type == resource_type(&dev.resources[i]) {
            if count == 0 {
                return Some(&mut dev.resources[i]);
            }
            count -= 1;
        }
    }
    
    None
}

/// platform_get_mem_or_io - get a memory or I/O resource for a device
/// @dev: platform device
/// @num: resource index
///
/// Return: a pointer to the resource or None on failure.
pub fn platform_get_mem_or_io(
    dev: &PlatformDevice,
    num: usize,
) -> Option<&Resource> {
    let mut count = num;
    
    for i in 0..dev.num_resources {
        let r = &dev.resources[i];
        
        if (resource_type(r) & (IORESOURCE_MEM | IORESOURCE_IO)) != 0 {
            if count == 0 {
                return Some(r);
            }
            count -= 1;
        }
    }
    
    None
}

/// platform_get_resource_byname - get a resource for a device by name
/// @dev: platform device
/// @type: resource type
/// @name: resource name
///
/// Return: a pointer to the resource or None on failure.
pub fn platform_get_resource_byname<'a>(
    dev: &'a PlatformDevice,
    res_type: u64,
    name: &str,
) -> Option<&'a Resource> {
    for i in 0..dev.num_resources {
        let r = &dev.resources[i];
        
        if let Some(res_name) = r.name {
            if res_type == resource_type(r) && res_name == name {
                return Some(r);
            }
        }
    }
    
    None
}

/// devm_platform_ioremap_resource - call devm_ioremap_resource() for a platform device
/// @pdev: platform device to use both for memory resource lookup as well as
///        resource management
/// @index: resource index
///
/// Return: a pointer to the remapped memory or an error on failure.
pub fn devm_platform_ioremap_resource(
    pdev: &PlatformDevice,
    index: usize,
) -> Result<MmioRegion> {
    use crate::drivers::resource::mmio::IoremapType;
    
    let r = platform_get_resource(pdev, IORESOURCE_MEM, index)
        .ok_or(PlatformError::NoDevice)?;
    
    // Map the MMIO region with default (uncached) mapping
    MmioRegion::new(r.start, r.size() as usize, IoremapType::Uncached)
        .map_err(|_| PlatformError::NoMemory)
}

/// devm_platform_get_and_ioremap_resource - call devm_ioremap_resource() for a
///                                          platform device and get resource
/// @pdev: platform device to use both for memory resource lookup as well as
///        resource management
/// @index: resource index
/// @res: optional output parameter to store a pointer to the obtained resource.
///
/// Return: a pointer to the remapped memory or an error on failure.
pub fn devm_platform_get_and_ioremap_resource(
    pdev: &PlatformDevice,
    index: usize,
) -> Result<(MmioRegion, &Resource)> {
    use crate::drivers::resource::mmio::IoremapType;
    
    let r = platform_get_resource(pdev, IORESOURCE_MEM, index)
        .ok_or(PlatformError::NoDevice)?;
    
    let region = MmioRegion::new(r.start, r.size() as usize, IoremapType::Uncached)
        .map_err(|_| PlatformError::NoMemory)?;
    
    Ok((region, r))
}

/// devm_platform_ioremap_resource_byname - call devm_ioremap_resource for
///                                         a platform device, retrieve the
///                                         resource by name
/// @pdev: platform device to use both for memory resource lookup as well as
///        resource management
/// @name: name of the resource
///
/// Return: a pointer to the remapped memory or an error on failure.
pub fn devm_platform_ioremap_resource_byname(
    pdev: &PlatformDevice,
    name: &str,
) -> Result<MmioRegion> {
    use crate::drivers::resource::mmio::IoremapType;
    
    let r = platform_get_resource_byname(pdev, IORESOURCE_MEM, name)
        .ok_or(PlatformError::NoDevice)?;
    
    MmioRegion::new(r.start, r.size() as usize, IoremapType::Uncached)
        .map_err(|_| PlatformError::NoMemory)
}

/// platform_get_irq_optional - get an optional interrupt for a device
/// @dev: platform device
/// @num: interrupt number index
///
/// Gets an interrupt for a platform device. Device drivers should check the
/// return value for errors so as to not pass a negative integer value to
/// the request_irq() APIs. This is the same as platform_get_irq(), except
/// that it does not print an error message if an interrupt can not be
/// obtained.
///
/// For example:
///     let irq = platform_get_irq_optional(pdev, 0)?;
///
/// Return: non-zero interrupt number on success, error on failure.
pub fn platform_get_irq_optional(
    dev: &PlatformDevice,
    num: usize,
) -> Result<u32> {
    // Try to get IRQ from Device Tree first
    if let Some(_of_node) = dev.of_node {
        // In real implementation, would call of_irq_get here
        // For now, fall through to resource lookup
    }
    
    // Get IRQ resource
    let r = platform_get_resource(dev, IORESOURCE_IRQ, num)
        .ok_or(PlatformError::NoDevice)?;
    
    if r.start == 0 {
        return Err(PlatformError::Invalid);
    }
    
    Ok(r.start as u32)
}

/// platform_get_irq - get an IRQ for a device
/// @dev: platform device
/// @num: IRQ number index
///
/// Gets an IRQ for a platform device and prints an error message if finding the
/// IRQ fails. Device drivers should check the return value for errors so as to
/// not pass a negative integer value to the request_irq() APIs.
///
/// For example:
///     let irq = platform_get_irq(pdev, 0)?;
///
/// Return: non-zero IRQ number on success, error on failure.
pub fn platform_get_irq(
    dev: &PlatformDevice,
    num: usize,
) -> Result<u32> {
    let ret = platform_get_irq_optional(dev, num);
    
    if ret.is_err() {
        // In real implementation, would print error message
        // pr_err!("IRQ index {} not found\n", num);
    }
    
    ret
}

/// platform_irq_count - Count the number of IRQs a platform device uses
/// @dev: platform device
///
/// Return: Number of IRQs a platform device uses or error
pub fn platform_irq_count(dev: &PlatformDevice) -> Result<usize> {
    let mut nr = 0;
    
    loop {
        match platform_get_irq_optional(dev, nr) {
            Ok(_) => nr += 1,
            Err(PlatformError::ProbeDefer) => return Err(PlatformError::ProbeDefer),
            Err(_) => break,
        }
    }
    
    Ok(nr)
}

/// __platform_get_irq_byname - internal function to get IRQ by name
fn __platform_get_irq_byname(
    dev: &PlatformDevice,
    name: &str,
) -> Result<u32> {
    // Try fwnode first (Device Tree or ACPI)
    // In real implementation, would call fwnode_irq_get_byname
    
    // Fall back to resource lookup
    let r = platform_get_resource_byname(dev, IORESOURCE_IRQ, name)
        .ok_or(PlatformError::NoDevice)?;
    
    if r.start == 0 {
        return Err(PlatformError::Invalid);
    }
    
    Ok(r.start as u32)
}

/// platform_get_irq_byname - get an IRQ for a device by name
/// @dev: platform device
/// @name: IRQ name
///
/// Get an IRQ like platform_get_irq(), but then by name rather then by index.
///
/// Return: non-zero IRQ number on success, error on failure.
pub fn platform_get_irq_byname(
    dev: &PlatformDevice,
    name: &str,
) -> Result<u32> {
    let ret = __platform_get_irq_byname(dev, name);
    
    if ret.is_err() {
        // In real implementation, would print error message
        // pr_err!("IRQ {} not found\n", name);
    }
    
    ret
}

/// platform_get_irq_byname_optional - get an optional IRQ for a device by name
/// @dev: platform device
/// @name: IRQ name
///
/// Get an optional IRQ by name like platform_get_irq_byname(). Except that it
/// does not print an error message if an IRQ can not be obtained.
///
/// Return: non-zero IRQ number on success, error on failure.
pub fn platform_get_irq_byname_optional(
    dev: &PlatformDevice,
    name: &str,
) -> Result<u32> {
    __platform_get_irq_byname(dev, name)
}

/// platform_device_alloc - create a platform device
/// @name: base name of the device we're adding
/// @id: instance id
///
/// Create a platform device object which can have other objects attached
/// to it, and which will have attached objects freed when it is released.
pub fn platform_device_alloc(name: String, id: i32) -> Option<Box<PlatformDevice>> {
    let mut pdev = Box::new(PlatformDevice::new(name, id));
    
    // Set up default DMA masks
    pdev.platform_dma_mask = 0xFFFFFFFF; // DMA_BIT_MASK(32)
    
    Some(pdev)
}

/// platform_device_put - destroy a platform device
/// @pdev: platform device to free
///
/// Free all memory associated with a platform device. This function must
/// _only_ be externally called in error cases. All other usage is a bug.
pub fn platform_device_put(pdev: Box<PlatformDevice>) {
    // Box will automatically drop and free memory
    drop(pdev);
}

/// platform_device_add_resources - add resources to a platform device
/// @pdev: platform device allocated by platform_device_alloc to add resources to
/// @res: set of resources that needs to be allocated for the device
/// @num: number of resources
///
/// Add a copy of the resources to the platform device. The memory
/// associated with the resources will be freed when the platform device is
/// released.
pub fn platform_device_add_resources(
    pdev: &mut PlatformDevice,
    res: &[Resource],
) -> Result<()> {
    // Clear existing resources
    pdev.resources.clear();
    
    // Copy new resources
    for r in res {
        pdev.resources.push(r.clone());
    }
    
    pdev.num_resources = pdev.resources.len();
    
    Ok(())
}

/// platform_device_add_data - add platform-specific data to a platform device
/// @pdev: platform device allocated by platform_device_alloc to add resources to
/// @data: platform specific data for this platform device
/// @size: size of platform specific data
///
/// Add a copy of platform specific data to the platform device's
/// platform_data pointer. The memory associated with the platform data
/// will be freed when the platform device is released.
pub fn platform_device_add_data(
    pdev: &mut PlatformDevice,
    data: *const u8,
    size: usize,
) -> Result<()> {
    if data.is_null() || size == 0 {
        pdev.platform_data = None;
        return Ok(());
    }
    
    // In real implementation, would allocate and copy data
    // For now, just store the pointer
    pdev.platform_data = Some(data as *mut u8);
    
    Ok(())
}

/// platform_device_add - add a platform device to device hierarchy
/// @pdev: platform device we're adding
///
/// This is part 2 of platform_device_register(), though may be called
/// separately _iff_ pdev was allocated by platform_device_alloc().
pub fn platform_device_add(pdev: &mut PlatformDevice) -> Result<()> {
    // Handle device ID allocation
    match pdev.id {
        PLATFORM_DEVID_AUTO => {
            // Automatically allocate device ID
            let id = PLATFORM_DEVID_IDA.fetch_add(1, Ordering::Relaxed);
            pdev.id = id as i32;
            pdev.id_auto = true;
        }
        PLATFORM_DEVID_NONE => {
            // No ID
        }
        _ => {
            // Explicit ID
        }
    }
    
    // Set resource names if not set
    for i in 0..pdev.num_resources {
        if pdev.resources[i].name.is_none() {
            // In real implementation, would set to device name
        }
    }
    
    // In real implementation, would:
    // 1. Insert resources into global resource tree
    // 2. Register device with device core
    // 3. Call device_add()
    
    Ok(())
}

/// platform_device_del - remove a platform-level device
/// @pdev: platform device we're removing
///
/// Note that this function will also release all memory- and port-based
/// resources owned by the device (@dev->resource). This function must
/// _only_ be externally called in error cases. All other usage is a bug.
pub fn platform_device_del(pdev: &mut PlatformDevice) {
    // Free auto-allocated ID
    if pdev.id_auto {
        pdev.id = PLATFORM_DEVID_AUTO;
    }
    
    // In real implementation, would:
    // 1. Call device_del()
    // 2. Release all resources
}

/// platform_device_register - add a platform-level device
/// @pdev: platform device we're adding
///
/// NOTE: _Never_ directly free @pdev after calling this function, even if it
/// returned an error! Always use platform_device_put() to give up the
/// reference initialised in this function instead.
pub fn platform_device_register(pdev: &mut PlatformDevice) -> Result<()> {
    // Set up DMA masks
    pdev.platform_dma_mask = 0xFFFFFFFF; // DMA_BIT_MASK(32)
    
    platform_device_add(pdev)
}

/// platform_device_unregister - unregister a platform-level device
/// @pdev: platform device we're unregistering
///
/// Unregistration is done in 2 steps. First we release all resources
/// and remove it from the subsystem, then we drop reference count by
/// calling platform_device_put().
pub fn platform_device_unregister(pdev: &mut PlatformDevice) {
    platform_device_del(pdev);
    // Note: In real implementation, would call platform_device_put()
    // but we can't consume pdev here since it's a mutable reference
}

/// platform_device_register_full - add a platform-level device with
/// resources and platform-specific data
///
/// @pdevinfo: data used to create device
///
/// Returns platform device on success, or error.
pub fn platform_device_register_full(
    pdevinfo: &PlatformDeviceInfo,
) -> Result<Box<PlatformDevice>> {
    let mut pdev = platform_device_alloc(pdevinfo.name.clone(), pdevinfo.id)
        .ok_or(PlatformError::NoMemory)?;
    
    // Set parent
    if let Some(parent) = pdevinfo.parent {
        pdev.dev.set_parent(parent);
    }
    
    // Set fwnode and of_node
    pdev.of_node = pdevinfo.fwnode;
    
    // Set DMA mask
    if pdevinfo.dma_mask != 0 {
        pdev.platform_dma_mask = pdevinfo.dma_mask;
    }
    
    // Add resources
    if let Some(ref res) = pdevinfo.res {
        platform_device_add_resources(&mut pdev, res)?;
    }
    
    // Add platform data
    if let Some(data) = pdevinfo.data {
        platform_device_add_data(&mut pdev, data, pdevinfo.size_data)?;
    }
    
    // Add device
    platform_device_add(&mut pdev)?;
    
    Ok(pdev)
}

/// platform_add_devices - add a numbers of platform devices
/// @devs: array of platform devices to add
/// @num: number of platform devices in array
///
/// Return: Ok on success, error on failure.
pub fn platform_add_devices(devs: &mut [&mut PlatformDevice]) -> Result<()> {
    for (i, dev) in devs.iter_mut().enumerate() {
        if let Err(e) = platform_device_register(dev) {
            // Unregister all previously registered devices
            for j in 0..i {
                platform_device_unregister(devs[j]);
            }
            return Err(e);
        }
    }
    
    Ok(())
}

/// __platform_driver_register - register a driver for platform-level devices
/// @drv: platform driver structure
///
/// Register a driver for platform-level devices.
pub fn platform_driver_register(_drv: &mut PlatformDriver) -> Result<()> {
    // In real implementation, would:
    // 1. Set driver.bus = &platform_bus_type
    // 2. Call driver_register(&drv->driver)
    
    Ok(())
}

/// platform_driver_unregister - unregister a driver for platform-level devices
/// @drv: platform driver structure
pub fn platform_driver_unregister(_drv: &mut PlatformDriver) {
    // In real implementation, would call driver_unregister(&drv->driver)
}

/// platform_match_id - match platform device ID
fn platform_match_id<'a>(
    id_table: &'a [PlatformDeviceId],
    pdev: &PlatformDevice,
) -> Option<&'a PlatformDeviceId> {
    for id in id_table {
        if id.name == pdev.name {
            return Some(id);
        }
    }
    None
}

/// platform_match - bind platform device to platform driver
/// @dev: device
/// @drv: driver
///
/// Platform device IDs are assumed to be encoded like this:
/// "<name><instance>", where <name> is a short description of the type of
/// device, like "pci" or "floppy", and <instance> is the enumerated
/// instance of the device, like '0' or '42'. Driver IDs are simply
/// "<name>". So, extract the <name> from the platform_device structure,
/// and compare it against the name of the driver. Return whether they match
/// or not.
pub fn platform_match(
    pdev: &PlatformDevice,
    pdrv: &PlatformDriver,
) -> bool {
    // Check driver_override first
    if let Some(ref override_name) = pdev.driver_override {
        return override_name == &pdrv.driver.name;
    }
    
    // Try Device Tree match (of_driver_match_device)
    // In real implementation, would check compatible strings
    
    // Try ACPI match
    // In real implementation, would check ACPI IDs
    
    // Try ID table match
    if let Some(id_table) = pdrv.id_table {
        if platform_match_id(id_table, pdev).is_some() {
            return true;
        }
    }
    
    // Fall back to driver name match
    pdev.name == pdrv.driver.name
}

/// platform_probe - probe a platform device
fn platform_probe(pdev: &mut PlatformDevice, pdrv: &PlatformDriver) -> Result<()> {
    // In real implementation, would:
    // 1. Set up clocks (of_clk_set_defaults)
    // 2. Attach PM domain (dev_pm_domain_attach)
    // 3. Call driver probe function
    
    if let Some(probe_fn) = pdrv.probe {
        probe_fn(pdev)?;
    }
    
    Ok(())
}

/// platform_remove - remove a platform device
fn platform_remove(pdev: &mut PlatformDevice, pdrv: &PlatformDriver) {
    if let Some(remove_fn) = pdrv.remove {
        remove_fn(pdev);
    }
}

/// platform_shutdown - shutdown a platform device
fn platform_shutdown(pdev: &mut PlatformDevice, pdrv: &PlatformDriver) {
    if let Some(shutdown_fn) = pdrv.shutdown {
        shutdown_fn(pdev);
    }
}

/// __platform_driver_probe - register driver for non-hotpluggable device
/// @drv: platform driver structure
/// @probe: the driver probe routine
///
/// Use this instead of platform_driver_register() when you know the device
/// is not hotpluggable and has already been registered, and you want to
/// remove its run-once probe() infrastructure from memory after the driver
/// has bound to the device.
///
/// One typical use for this would be with drivers for controllers integrated
/// into system-on-chip processors, where the controller devices have been
/// configured as part of board setup.
///
/// Note that this is incompatible with deferred probing.
///
/// Returns Ok if the driver registered and bound to a device, else returns
/// an error.
pub fn platform_driver_probe(
    drv: &mut PlatformDriver,
    probe: fn(&mut PlatformDevice) -> Result<()>,
) -> Result<()> {
    // Prevent deferred probe
    drv.prevent_deferred_probe = true;
    
    // Set probe function
    drv.probe = Some(probe);
    
    // Register driver
    platform_driver_register(drv)?;
    
    // In real implementation, would:
    // 1. Walk all platform devices
    // 2. Check if any bound to this driver
    // 3. If not, unregister and return error
    
    Ok(())
}

/// __platform_create_bundle - register driver and create corresponding device
/// @driver: platform driver structure
/// @probe: the driver probe routine
/// @res: set of resources that needs to be allocated for the device
/// @data: platform specific data for this platform device
/// @size: size of platform specific data
///
/// Use this in legacy-style modules that probe hardware directly and
/// register a single platform device and corresponding platform driver.
///
/// Returns platform device on success, or error.
pub fn platform_create_bundle(
    driver: &mut PlatformDriver,
    probe: fn(&mut PlatformDevice) -> Result<()>,
    res: Option<Vec<Resource>>,
    data: Option<*const u8>,
    size: usize,
) -> Result<Box<PlatformDevice>> {
    let mut pdev = platform_device_alloc(driver.driver.name.to_string(), PLATFORM_DEVID_NONE)
        .ok_or(PlatformError::NoMemory)?;
    
    // Add resources
    if let Some(resources) = res {
        platform_device_add_resources(&mut pdev, &resources)?;
    }
    
    // Add data
    if let Some(d) = data {
        platform_device_add_data(&mut pdev, d, size)?;
    }
    
    // Add device
    platform_device_add(&mut pdev)?;
    
    // Probe driver
    platform_driver_probe(driver, probe)?;
    
    Ok(pdev)
}

/// __platform_register_drivers - register an array of platform drivers
/// @drivers: an array of drivers to register
///
/// Registers platform drivers specified by an array. On failure to register a
/// driver, all previously registered drivers will be unregistered.
///
/// Returns: Ok on success or error on failure.
pub fn platform_register_drivers(drivers: &mut [&mut PlatformDriver]) -> Result<()> {
    for (i, drv) in drivers.iter_mut().enumerate() {
        if let Err(e) = platform_driver_register(drv) {
            // Unregister all previously registered drivers
            for j in 0..i {
                platform_driver_unregister(drivers[j]);
            }
            return Err(e);
        }
    }
    
    Ok(())
}

/// platform_unregister_drivers - unregister an array of platform drivers
/// @drivers: an array of drivers to unregister
///
/// Unregisters platform drivers specified by an array. Drivers are
/// unregistered in the reverse order in which they were registered.
pub fn platform_unregister_drivers(drivers: &mut [&mut PlatformDriver]) {
    for drv in drivers.iter_mut().rev() {
        platform_driver_unregister(drv);
    }
}

/// platform_find_device_by_driver - Find a platform device with a given driver
/// @start: The device to start the search from
/// @drv: The device driver to look for
///
/// Find a platform device that matches the given driver.
pub fn platform_find_device_by_driver<'a>(
    _start: Option<&'a PlatformDevice>,
    _drv: &PlatformDriver,
) -> Option<&'a PlatformDevice> {
    // In real implementation, would:
    // 1. Call bus_find_device(&platform_bus_type, start, drv, __platform_match)
    // 2. Return matching device
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::resource::core::Resource;

    #[test]
    fn test_platform_device_new() {
        let pdev = PlatformDevice::new("test-device".to_string(), 0);
        assert_eq!(pdev.name(), "test-device");
        assert_eq!(pdev.id(), 0);
        assert_eq!(pdev.num_resources, 0);
    }

    #[test]
    fn test_platform_device_alloc() {
        let pdev = platform_device_alloc("test-device".to_string(), 0);
        assert!(pdev.is_some());
        let pdev = pdev.unwrap();
        assert_eq!(pdev.name(), "test-device");
        assert_eq!(pdev.platform_dma_mask, 0xFFFFFFFF);
    }

    #[test]
    fn test_platform_device_add_resources() {
        let mut pdev = PlatformDevice::new("test-device".to_string(), 0);
        
        let resources = vec![
            Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM),
            Resource::new(0x2000, 0x2FFF, IORESOURCE_MEM),
            Resource::new(10, 10, IORESOURCE_IRQ),
        ];
        
        let result = platform_device_add_resources(&mut pdev, &resources);
        assert!(result.is_ok());
        assert_eq!(pdev.num_resources, 3);
    }

    #[test]
    fn test_platform_get_resource() {
        let mut pdev = PlatformDevice::new("test-device".to_string(), 0);
        
        let resources = vec![
            Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM),
            Resource::new(0x2000, 0x2FFF, IORESOURCE_MEM),
            Resource::new(10, 10, IORESOURCE_IRQ),
        ];
        
        platform_device_add_resources(&mut pdev, &resources).unwrap();
        
        // Get first MEM resource
        let res = platform_get_resource(&pdev, IORESOURCE_MEM, 0);
        assert!(res.is_some());
        assert_eq!(res.unwrap().start, 0x1000);
        
        // Get second MEM resource
        let res = platform_get_resource(&pdev, IORESOURCE_MEM, 1);
        assert!(res.is_some());
        assert_eq!(res.unwrap().start, 0x2000);
        
        // Get IRQ resource
        let res = platform_get_resource(&pdev, IORESOURCE_IRQ, 0);
        assert!(res.is_some());
        assert_eq!(res.unwrap().start, 10);
        
        // Get non-existent resource
        let res = platform_get_resource(&pdev, IORESOURCE_MEM, 2);
        assert!(res.is_none());
    }

    #[test]
    fn test_platform_get_mem_or_io() {
        let mut pdev = PlatformDevice::new("test-device".to_string(), 0);
        
        let resources = vec![
            Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM),
            Resource::new(10, 10, IORESOURCE_IRQ),
            Resource::new(0x2000, 0x2FFF, IORESOURCE_IO),
        ];
        
        platform_device_add_resources(&mut pdev, &resources).unwrap();
        
        // Get first MEM/IO resource (MEM)
        let res = platform_get_mem_or_io(&pdev, 0);
        assert!(res.is_some());
        assert_eq!(res.unwrap().start, 0x1000);
        
        // Get second MEM/IO resource (IO)
        let res = platform_get_mem_or_io(&pdev, 1);
        assert!(res.is_some());
        assert_eq!(res.unwrap().start, 0x2000);
        
        // IRQ should be skipped
        let res = platform_get_mem_or_io(&pdev, 2);
        assert!(res.is_none());
    }

    #[test]
    fn test_platform_get_resource_byname() {
        let mut pdev = PlatformDevice::new("test-device".to_string(), 0);
        
        let mut res1 = Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM);
        res1.name = Some("mem1");
        
        let mut res2 = Resource::new(0x2000, 0x2FFF, IORESOURCE_MEM);
        res2.name = Some("mem2");
        
        let resources = vec![res1, res2];
        
        platform_device_add_resources(&mut pdev, &resources).unwrap();
        
        // Get resource by name
        let res = platform_get_resource_byname(&pdev, IORESOURCE_MEM, "mem1");
        assert!(res.is_some());
        assert_eq!(res.unwrap().start, 0x1000);
        
        let res = platform_get_resource_byname(&pdev, IORESOURCE_MEM, "mem2");
        assert!(res.is_some());
        assert_eq!(res.unwrap().start, 0x2000);
        
        // Non-existent name
        let res = platform_get_resource_byname(&pdev, IORESOURCE_MEM, "mem3");
        assert!(res.is_none());
    }

    #[test]
    fn test_platform_get_irq_optional() {
        let mut pdev = PlatformDevice::new("test-device".to_string(), 0);
        
        let resources = vec![
            Resource::new(10, 10, IORESOURCE_IRQ),
            Resource::new(11, 11, IORESOURCE_IRQ),
        ];
        
        platform_device_add_resources(&mut pdev, &resources).unwrap();
        
        // Get first IRQ
        let irq = platform_get_irq_optional(&pdev, 0);
        assert!(irq.is_ok());
        assert_eq!(irq.unwrap(), 10);
        
        // Get second IRQ
        let irq = platform_get_irq_optional(&pdev, 1);
        assert!(irq.is_ok());
        assert_eq!(irq.unwrap(), 11);
        
        // Non-existent IRQ
        let irq = platform_get_irq_optional(&pdev, 2);
        assert!(irq.is_err());
    }

    #[test]
    fn test_platform_irq_count() {
        let mut pdev = PlatformDevice::new("test-device".to_string(), 0);
        
        let resources = vec![
            Resource::new(10, 10, IORESOURCE_IRQ),
            Resource::new(11, 11, IORESOURCE_IRQ),
            Resource::new(12, 12, IORESOURCE_IRQ),
        ];
        
        platform_device_add_resources(&mut pdev, &resources).unwrap();
        
        let count = platform_irq_count(&pdev);
        assert!(count.is_ok());
        assert_eq!(count.unwrap(), 3);
    }

    #[test]
    fn test_platform_device_auto_id() {
        let mut pdev = PlatformDevice::new("test-device".to_string(), PLATFORM_DEVID_AUTO);
        
        let result = platform_device_add(&mut pdev);
        assert!(result.is_ok());
        assert!(pdev.id_auto);
        assert!(pdev.id >= 0);
    }

    #[test]
    fn test_platform_match() {
        let pdev = PlatformDevice::new("test-device".to_string(), 0);
        let mut pdrv = PlatformDriver::new("test-device");
        
        // Should match by name
        assert!(platform_match(&pdev, &pdrv));
        
        // Should not match different name
        let pdrv2 = PlatformDriver::new("other-device");
        assert!(!platform_match(&pdev, &pdrv2));
    }

    #[test]
    fn test_platform_match_with_id_table() {
        let pdev = PlatformDevice::new("test-device".to_string(), 0);
        
        let id_table = [
            PlatformDeviceId {
                name: "test-device",
                driver_data: 0,
            },
        ];
        
        let mut pdrv = PlatformDriver::new("other-name");
        pdrv.id_table = Some(&id_table);
        
        // Should match via ID table
        assert!(platform_match(&pdev, &pdrv));
    }

    #[test]
    fn test_platform_driver_override() {
        let mut pdev = PlatformDevice::new("test-device".to_string(), 0);
        pdev.driver_override = Some("specific-driver".to_string());
        
        let mut pdrv = PlatformDriver::new("specific-driver");
        
        // Should match via driver_override
        assert!(platform_match(&pdev, &pdrv));
        
        // Should not match other drivers
        let pdrv2 = PlatformDriver::new("other-driver");
        assert!(!platform_match(&pdev, &pdrv2));
    }

    #[test]
    fn test_platform_add_devices() {
        let mut pdev1 = PlatformDevice::new("device1".to_string(), 0);
        let mut pdev2 = PlatformDevice::new("device2".to_string(), 1);
        
        let mut devs = [&mut pdev1, &mut pdev2];
        
        let result = platform_add_devices(&mut devs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_platform_device_register_full() {
        let resources = vec![
            Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM),
        ];
        
        let pdevinfo = PlatformDeviceInfo {
            parent: None,
            fwnode: None,
            name: "test-device".to_string(),
            id: 0,
            res: Some(resources),
            num_res: 1,
            data: None,
            size_data: 0,
            dma_mask: 0xFFFFFFFF,
            properties: None,
            of_node_reused: false,
        };
        
        let result = platform_device_register_full(&pdevinfo);
        assert!(result.is_ok());
        
        let pdev = result.unwrap();
        assert_eq!(pdev.name(), "test-device");
        assert_eq!(pdev.num_resources, 1);
    }
}
