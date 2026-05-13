//! Linux-style driver registration examples
//! 
//! Shows how to register drivers that automatically match devices

#![allow(dead_code)]

use crate::drivers::*;
use crate::prelude::*;

/// Example: PCI network card driver (like Linux e1000 driver)
pub fn example_pci_network_driver() {
    let driver = Arc::new(
        Driver::new("e1000_compat", BusType::PCI)
            .with_ids(vec![
                // Intel 82540EM Gigabit Ethernet
                ExtendedDeviceId::new(0x8086, 0x100E),
                // Intel 82545EM Gigabit Ethernet
                ExtendedDeviceId::new(0x8086, 0x100F),
                // Intel 82574L Gigabit Network
                ExtendedDeviceId::new(0x8086, 0x10D3),
            ])
            .with_probe_type(ProbeType::Asynchronous)
            .with_probe(|dev| {
                // Initialize network card
                dev.init()?;
                // println!("Network card {} initialized", dev.name());
                Ok(())
            })
            .with_remove(|dev| {
                // println!("Network card {} removed", dev.name());
                dev.shutdown()
            })
    );
    
    // Register driver - will automatically match any Intel network cards
    global_driver_manager().register_driver(driver).unwrap();
}

/// Example: USB storage driver (like Linux usb-storage)
pub fn example_usb_storage_driver() {
    let driver = Arc::new(
        Driver::new("usb_storage", BusType::USB)
            .with_ids(vec![
                // USB Mass Storage Class
                ExtendedDeviceId::with_class(
                    ExtendedDeviceId::ANY,
                    ExtendedDeviceId::ANY,
                    0x080650, // Class 08 (Mass Storage), Subclass 06 (SCSI), Protocol 50 (Bulk-Only)
                    0xFFFFFF,
                ),
            ])
            .with_probe_type(ProbeType::Synchronous)
            .with_probe(|dev| {
                dev.init()?;
                // println!("USB storage {} mounted", dev.name());
                Ok(())
            })
    );
    
    global_driver_manager().register_driver(driver).unwrap();
}

/// Example: I2C sensor driver (like Linux bmp280)
pub fn example_i2c_sensor_driver() {
    let driver = Arc::new(
        Driver::new("bmp280_sensor", BusType::I2C)
            .with_ids(vec![
                // Bosch BMP280 pressure sensor
                ExtendedDeviceId::new(0x0077, 0x0280),
                // Bosch BME280 humidity sensor
                ExtendedDeviceId::new(0x0077, 0x0680),
            ])
            .with_probe_type(ProbeType::Asynchronous)
            .with_probe(|dev| {
                dev.init()?;
                // println!("Sensor {} ready", dev.name());
                Ok(())
            })
    );
    
    global_driver_manager().register_driver(driver).unwrap();
}

/// Example: Wildcard driver (matches any device from vendor)
pub fn example_wildcard_driver() {
    let driver = Arc::new(
        Driver::new("generic_qualcomm", BusType::Platform)
            .with_ids(vec![
                // Any Qualcomm device
                ExtendedDeviceId::new(0x17CB, ExtendedDeviceId::ANY),
            ])
            .with_probe(|dev| {
                dev.init()?;
                // println!("Qualcomm device {} initialized", dev.name());
                Ok(())
            })
    );
    
    global_driver_manager().register_driver(driver).unwrap();
}

/// Example: Hot-plug scenario - device appears, driver auto-matches
pub fn example_hotplug_scenario() {
    // 1. Register drivers first
    example_usb_storage_driver();
    example_i2c_sensor_driver();
    
    // 2. Device appears (USB flash drive)
    let usb_device_id = ExtendedDeviceId::with_class(
        ExtendedDeviceId::ANY,
        ExtendedDeviceId::ANY,
        0x080650,
        0xFFFFFF,
    );
    
    let device = Arc::new(spin::RwLock::new(MockDevice::new(MockConfig {
        device_id: usb_device_id.to_simple(),
        name: "usb_flash",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: true,
            supports_dma: true,
            hot_pluggable: true,
        },
        fail_init: false,
        fail_shutdown: false,
    })));
    
    // 3. Add device - driver manager will automatically find and probe driver
    global_driver_manager().add_device(usb_device_id, device).unwrap();
    
    // println!("Device auto-matched and initialized!");
}

/// Example: List all registered drivers
pub fn example_list_drivers() {
    let drivers = global_driver_manager().list_drivers();
    // println!("Registered drivers: {}", drivers.len());
    for _driver in drivers {
        // println!("  - {}", driver);
    }
}

/// Example: Complete system initialization (like Linux boot)
pub fn example_system_init() {
    // println!("=== StarOS Driver System Init ===");
    
    // 1. Register all drivers
    // println!("Registering drivers...");
    example_pci_network_driver();
    example_usb_storage_driver();
    example_i2c_sensor_driver();
    example_wildcard_driver();
    
    // println!("Drivers registered: {}", global_driver_manager().driver_count());
    
    // 2. Scan buses for devices
    // println!("Scanning buses...");
    // (Bus scanning will be implemented in Phase II)
    
    // 3. Devices will auto-match with drivers
    // println!("Auto-matching devices...");
    
    // println!("=== System Ready ===");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_registration() {
        let manager = DriverManager::new();
        
        let driver = Arc::new(Driver::new("test", BusType::PCI)
            .with_ids(vec![ExtendedDeviceId::new(0x1234, 0x5678)]));
        
        assert!(manager.register_driver(driver).is_ok());
        assert_eq!(manager.driver_count(), 1);
    }
}
