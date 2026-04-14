//! Ghost Bus Usage Example
//! 
//! This example demonstrates how to use the Ghost Bus architecture
//! to register and manage devices.

#![allow(dead_code)]

use crate::drivers::*;
use crate::prelude::*;
use spin::RwLock;

/// Example: Register a mock sensor device
pub fn example_register_sensor() -> Result<(), RegistryError> {
    // Create device configuration
    let config = MockConfig {
        device_id: DeviceId::new(0x1234, 0x5678, 0),
        name: "accelerometer",
        capabilities: DeviceCapabilities {
            can_stream: true,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: true,
        },
        fail_init: false,
        fail_shutdown: false,
    };
    
    // Create device instance
    let device = MockStreamDevice::new(config, 1000); // 1000 Hz sample rate
    
    // Wrap in Arc<RwLock<>> for thread-safe sharing
    let handle = Arc::new(RwLock::new(device));
    
    // Register with global registry
    let registry = global_registry();
    registry.register(DeviceId::new(0x1234, 0x5678, 0), handle)?;
    
    Ok(())
}

/// Example: Lookup and use a device
pub fn example_use_device() -> Option<()> {
    let registry = global_registry();
    let device_id = DeviceId::new(0x1234, 0x5678, 0);
    
    // Lookup device
    let handle = registry.lookup(device_id)?;
    
    // Lock and use device
    let mut device = handle.write();
    
    // Initialize device
    device.init().ok()?;
    
    // Use device
    println!("Device: {}", device.name());
    println!("ID: {}", device.device_id());
    
    Some(())
}

/// Example: Hot-plug simulation
pub fn example_hotplug() -> Result<(), RegistryError> {
    let registry = global_registry();
    let device_id = DeviceId::new(0xABCD, 0xEF01, 0);
    
    // Device plugged in
    let config = MockConfig {
        device_id,
        name: "usb_device",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: true,
            supports_dma: true,
            hot_pluggable: true,
        },
        fail_init: false,
        fail_shutdown: false,
    };
    
    let device = Arc::new(RwLock::new(MockDevice::new(config)));
    registry.register(device_id, device)?;
    
    println!("Device plugged in: {}", device_id);
    
    // ... use device ...
    
    // Device unplugged
    registry.unregister(device_id)?;
    println!("Device unplugged: {}", device_id);
    
    Ok(())
}

/// Example: List all devices
pub fn example_list_devices() {
    let registry = global_registry();
    let devices = registry.list_devices();
    
    println!("Registered devices: {}", devices.len());
    for device_id in devices {
        println!("  - {}", device_id);
    }
}

/// Example: Stream data from sensor
pub fn example_stream_sensor() -> Option<()> {
    let registry = global_registry();
    let device_id = DeviceId::new(0x1234, 0x5678, 0);
    
    let handle = registry.lookup(device_id)?;
    let mut device = handle.write();
    
    // Start streaming
    if let Ok(streamable) = device.init() {
        // Note: In real implementation, would downcast to Streamable
        println!("Streaming from: {}", device.name());
    }
    
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example_register() {
        // Note: This test would work with proper allocator
        // Currently disabled due to DummyAllocator
    }
}
