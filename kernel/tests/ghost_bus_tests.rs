//! Ghost Bus Integration Tests
//! 
//! Tests for the universal driver subsystem

use staros_kernel::drivers::*;
use std::sync::Arc;
use spin::RwLock;

#[test]
fn test_device_id_creation() {
    let id = DeviceId::new(0x1234, 0x5678, 0);
    assert_eq!(id.vendor, 0x1234);
    assert_eq!(id.product, 0x5678);
    assert_eq!(id.instance, 0);
}

#[test]
fn test_device_id_display() {
    let id = DeviceId::new(0xABCD, 0xEF01, 5);
    assert_eq!(format!("{}", id), "abcd:ef01:05");
}

#[test]
fn test_mock_device_lifecycle() {
    let config = MockConfig {
        device_id: DeviceId::new(0x1000, 0x2000, 0),
        name: "test_device",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: true,
        },
        fail_init: false,
        fail_shutdown: false,
    };
    
    let mut device = MockDevice::new(config);
    
    // Test initialization
    assert!(device.init().is_ok());
    assert_eq!(device.name(), "test_device");
    
    // Test power management
    assert!(device.power_save(PowerState::Suspend).is_ok());
    assert!(device.power_save(PowerState::Active).is_ok());
    
    // Test shutdown
    assert!(device.shutdown().is_ok());
}

#[test]
fn test_mock_device_failure_modes() {
    let config = MockConfig {
        device_id: DeviceId::new(0xFFFF, 0xFFFF, 0),
        name: "fail_device",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: false,
        },
        fail_init: true,
        fail_shutdown: false,
    };
    
    let mut device = MockDevice::new(config);
    assert!(device.init().is_err());
}

#[test]
fn test_registry_register_and_lookup() {
    let registry = DeviceRegistry::new();
    let id = DeviceId::new(0x1000, 0x2000, 0);
    
    let device = Arc::new(RwLock::new(MockDevice::new(MockConfig {
        device_id: id,
        name: "test",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: true,
        },
        fail_init: false,
        fail_shutdown: false,
    })));
    
    // Register device
    assert!(registry.register(id, device).is_ok());
    assert_eq!(registry.count(), 1);
    
    // Lookup device
    let found = registry.lookup(id);
    assert!(found.is_some());
    
    // Verify device properties
    let handle = found.unwrap();
    let dev = handle.read();
    assert_eq!(dev.device_id(), id);
    assert_eq!(dev.name(), "test");
}

#[test]
fn test_registry_duplicate_prevention() {
    let registry = DeviceRegistry::new();
    let id = DeviceId::new(0x1000, 0x2000, 0);
    
    let make_config = || MockConfig {
        device_id: id,
        name: "dup_test",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: false,
        },
        fail_init: false,
        fail_shutdown: false,
    };
    
    let device1 = Arc::new(RwLock::new(MockDevice::new(make_config())));
    let device2 = Arc::new(RwLock::new(MockDevice::new(make_config())));
    
    assert!(registry.register(id, device1).is_ok());
    assert_eq!(registry.register(id, device2), Err(RegistryError::DuplicateDevice));
}

#[test]
fn test_registry_unregister() {
    let registry = DeviceRegistry::new();
    let id = DeviceId::new(0x1000, 0x2000, 0);
    
    let config = MockConfig {
        device_id: id,
        name: "unreg_test",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: false,
        },
        fail_init: false,
        fail_shutdown: false,
    };
    
    let device = Arc::new(RwLock::new(MockDevice::new(config)));
    
    registry.register(id, device).unwrap();
    assert_eq!(registry.count(), 1);
    
    let removed = registry.unregister(id);
    assert!(removed.is_ok());
    assert_eq!(registry.count(), 0);
    assert!(registry.lookup(id).is_none());
}

#[test]
fn test_registry_list_devices() {
    let registry = DeviceRegistry::new();
    
    let id1 = DeviceId::new(0x1000, 0x2000, 0);
    let id2 = DeviceId::new(0x1000, 0x2000, 1);
    let id3 = DeviceId::new(0x3000, 0x4000, 0);
    
    let make_config = || MockConfig {
        device_id: DeviceId::new(0, 0, 0),
        name: "list_test",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: false,
        },
        fail_init: false,
        fail_shutdown: false,
    };
    
    registry.register(id1, Arc::new(RwLock::new(MockDevice::new(make_config())))).unwrap();
    registry.register(id2, Arc::new(RwLock::new(MockDevice::new(make_config())))).unwrap();
    registry.register(id3, Arc::new(RwLock::new(MockDevice::new(make_config())))).unwrap();
    
    let devices = registry.list_devices();
    assert_eq!(devices.len(), 3);
    assert!(devices.contains(&id1));
    assert!(devices.contains(&id2));
    assert!(devices.contains(&id3));
}

#[test]
fn test_mock_stream_device() {
    let config = MockConfig {
        device_id: DeviceId::new(0x2000, 0x3000, 0),
        name: "stream_test",
        capabilities: DeviceCapabilities {
            can_stream: true,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: false,
        },
        fail_init: false,
        fail_shutdown: false,
    };
    
    let mut device = MockStreamDevice::new(config, 1000);
    
    // Initialize
    device.init().unwrap();
    
    // Start streaming
    assert!(device.start_stream().is_ok());
    assert_eq!(device.sample_rate(), 1000);
    
    // Poll data
    let data1 = device.poll().unwrap();
    assert!(data1.is_some());
    
    let data2 = device.poll().unwrap();
    assert!(data2.is_some());
    
    // Values should increment
    assert_eq!(data1.unwrap().value, 0.0);
    assert_eq!(data2.unwrap().value, 0.1);
    
    // Stop streaming
    assert!(device.stop_stream().is_ok());
    
    // Should return None when not streaming
    let data3 = device.poll().unwrap();
    assert!(data3.is_none());
}

#[test]
fn test_hot_plug_simulation() {
    let registry = DeviceRegistry::new();
    
    // Simulate device insertion
    let id = DeviceId::new(0x1234, 0x5678, 0);
    let device = Arc::new(RwLock::new(MockDevice::new(MockConfig {
        device_id: id,
        name: "hotplug_device",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: true,
        },
        fail_init: false,
        fail_shutdown: false,
    })));
    
    // Register (plug in)
    let start = std::time::Instant::now();
    registry.register(id, device).unwrap();
    let register_time = start.elapsed();
    
    // Should be very fast (<1ms target)
    println!("Registration time: {:?}", register_time);
    assert!(register_time.as_micros() < 1000, "Registration took too long: {:?}", register_time);
    
    // Verify device is accessible
    assert!(registry.lookup(id).is_some());
    
    // Unregister (unplug)
    let start = std::time::Instant::now();
    registry.unregister(id).unwrap();
    let unregister_time = start.elapsed();
    
    println!("Unregistration time: {:?}", unregister_time);
    assert!(unregister_time.as_micros() < 1000, "Unregistration took too long: {:?}", unregister_time);
    
    // Verify device is gone
    assert!(registry.lookup(id).is_none());
}

#[test]
fn test_concurrent_device_access() {
    use std::thread;
    
    let registry = Arc::new(DeviceRegistry::new());
    let id = DeviceId::new(0x1000, 0x2000, 0);
    
    let config = MockConfig {
        device_id: id,
        name: "concurrent_test",
        capabilities: DeviceCapabilities {
            can_stream: false,
            can_block_io: false,
            supports_dma: false,
            hot_pluggable: false,
        },
        fail_init: false,
        fail_shutdown: false,
    };
    
    let device = Arc::new(RwLock::new(MockDevice::new(config)));
    registry.register(id, device).unwrap();
    
    let mut handles = vec![];
    
    // Spawn multiple threads accessing the same device
    for i in 0..10 {
        let reg = Arc::clone(&registry);
        let handle = thread::spawn(move || {
            for _ in 0..100 {
                if let Some(dev_handle) = reg.lookup(id) {
                    let dev = dev_handle.read();
                    assert_eq!(dev.device_id(), id);
                }
            }
            i
        });
        handles.push(handle);
    }
    
    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
}
