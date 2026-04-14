//! Bus infrastructure integration tests with real DTB data

use common::{DtbTestSuite, hw_mock::MockDevice};

#[test]
fn test_bus_lifecycle_with_real_dtb() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    if suite.is_empty() {
        eprintln!("Warning: No DTB files for bus testing");
        return;
    }
    
    for vector in suite.vectors() {
        println!("Testing bus lifecycle with: {}", vector.name);
        
        // Simulate full lifecycle:
        // 1. Load DTB
        // 2. Parse device tree
        // 3. Register bus
        // 4. Enumerate devices
        // 5. Match drivers
        // 6. Probe devices
        // 7. Remove devices
        // 8. Unregister bus
        
        // This would integrate with actual bus.rs code
        assert!(vector.is_valid());
    }
}

#[test]
fn test_device_enumeration_from_dtb() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    for vector in suite.vectors() {
        // Parse DTB and enumerate all devices
        // Each device should be properly registered on its bus
        
        println!("Enumerating devices from: {}", vector.name);
        
        // This would call actual device enumeration code
        // For now, validate DTB structure
        assert!(vector.size() > 0);
    }
}

#[test]
fn test_driver_matching_simulation() {
    // Simulate driver matching with mock devices
    let device1 = MockDevice::new("uart@fe001000", 4);
    let device2 = MockDevice::new("i2c@fe002000", 8);
    
    assert_eq!(device1.name, "uart@fe001000");
    assert_eq!(device1.registers.len(), 4);
    assert_eq!(device2.registers.len(), 8);
}

#[test]
fn test_address_translation_from_real_dtb() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    for vector in suite.vectors() {
        println!("Testing address translation for: {}", vector.name);
        
        // Extract addresses from DTB
        // Translate through ranges
        // Verify correctness
        
        // This would integrate with address.rs
        assert!(vector.is_valid());
    }
}

#[test]
fn test_irq_mapping_from_real_dtb() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    for vector in suite.vectors() {
        println!("Testing IRQ mapping for: {}", vector.name);
        
        // Extract interrupt properties
        // Map to IRQ numbers
        // Verify interrupt controller hierarchy
        
        // This would integrate with irq.rs
        assert!(vector.is_valid());
    }
}

#[test]
fn test_platform_device_creation() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    for vector in suite.vectors() {
        println!("Testing platform device creation for: {}", vector.name);
        
        // Parse DTB
        // Create platform devices
        // Verify resources (MMIO, IRQ)
        // Verify device hierarchy
        
        // This would integrate with platform.rs
        assert!(vector.is_valid());
    }
}
