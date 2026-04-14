// Ghost Bus standalone test
// Run with: cargo test --bin ghost_bus_test

fn main() {
    println!("Ghost Bus Test Suite");
    
    test_device_id();
    test_mock_device();
    test_registry();
    
    println!("\n✅ All Ghost Bus tests passed!");
}

fn test_device_id() {
    use staros_kernel::drivers::DeviceId;
    
    let id = DeviceId::new(0x1234, 0x5678, 0);
    assert_eq!(id.vendor, 0x1234);
    assert_eq!(id.product, 0x5678);
    assert_eq!(format!("{}", id), "1234:5678:00");
    
    println!("✓ DeviceId tests passed");
}

fn test_mock_device() {
    use staros_kernel::drivers::*;
    
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
    
    assert!(device.init().is_ok());
    assert_eq!(device.name(), "test_device");
    assert!(device.shutdown().is_ok());
    
    println!("✓ MockDevice tests passed");
}

fn test_registry() {
    use staros_kernel::drivers::*;
    use std::sync::Arc;
    use spin::RwLock;
    
    let registry = DeviceRegistry::new();
    let id = DeviceId::new(0x1000, 0x2000, 0);
    
    let config = MockConfig {
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
    };
    
    let device = Arc::new(RwLock::new(MockDevice::new(config)));
    
    assert!(registry.register(id, device).is_ok());
    assert_eq!(registry.count(), 1);
    assert!(registry.lookup(id).is_some());
    
    println!("✓ Registry tests passed");
}
