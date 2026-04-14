// SPDX-License-Identifier: GPL-2.0
/*
 * Driver base infrastructure
 *
 * Ported from Linux drivers/base/
 */

pub mod bus;
pub mod device;
pub mod driver;
pub mod platform;

pub use bus::{
    BusType, Device, DeviceDriver, BusError,
    bus_register, bus_unregister,
    bus_add_device, bus_remove_device,
    bus_add_driver, bus_remove_driver,
    bus_for_each_dev, bus_for_each_drv,
    bus_probe_device, bus_rescan_devices,
    bus_is_registered,
    device_attach,
};

pub use device::{
    DeviceCore, DeviceError, DeviceHandle,
    device_initialize, device_add, device_register,
    device_del, device_unregister,
    get_device, put_device,
    device_for_each_child, device_is_registered,
};

pub use driver::{
    DriverCore, DriverError,
    driver_register, driver_unregister,
    driver_for_each_device, driver_find_device,
    driver_attach, driver_detach,
};

pub use platform::{
    PlatformDevice, PlatformDriver, PlatformDeviceId, PlatformDeviceInfo,
    PlatformError,
    platform_get_resource, platform_get_resource_byname,
    platform_get_irq, platform_get_irq_optional, platform_get_irq_byname,
    platform_device_alloc, platform_device_register, platform_device_unregister,
    platform_driver_register, platform_driver_unregister,
    platform_add_devices,
};
