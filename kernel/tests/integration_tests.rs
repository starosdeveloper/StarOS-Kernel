use kernel::boot_integration::{boot_kernel, early_debug_init};
use kernel::safety::boot_validator::BootValidator;
use kernel::devicetree::parser::FdtParser;

#[test]
fn test_boot_sequence_integration() {
    // This test requires a valid DTB address
    // In real environment, this would be provided by bootloader
}

#[test]
fn test_early_uart_probe() {
    let uart = early_debug_init();
    if let Some(uart) = uart {
        assert!(uart.base_addr() > 0);
    }
}

#[test]
fn test_dtb_validation() {
    // Test with invalid address
    assert!(BootValidator::validate_dtb(0).is_err());
    
    // Test with unaligned address
    assert!(BootValidator::validate_dtb(0x40000001).is_err());
}

#[test]
fn test_memory_map_validation() {
    assert!(BootValidator::validate_memory_map().is_ok());
}

#[cfg(test)]
mod device_tree_tests {
    use super::*;
    use kernel::devicetree::discovery::DeviceDiscovery;

    #[test]
    fn test_device_discovery_creation() {
        // Requires valid FDT
    }
}

#[cfg(test)]
mod soc_tests {
    use super::*;
    use kernel::soc::detection::SocInfo;

    #[test]
    fn test_soc_detection() {
        // Requires valid FDT
    }
}

#[cfg(test)]
mod boot_image_tests {
    use kernel::boot_integration::{BootImage, BootImageVersion};

    #[test]
    fn test_boot_image_v2_creation() {
        let kernel = vec![0u8; 4096];
        let dtb = vec![0u8; 1024];
        let img = BootImage::new(kernel, dtb, BootImageVersion::V2);
        let packed = img.pack().unwrap();
        assert!(packed.len() > 0);
        assert_eq!(&packed[0..8], b"ANDROID!");
    }

    #[test]
    fn test_boot_image_v3_creation() {
        let kernel = vec![0u8; 4096];
        let dtb = vec![0u8; 1024];
        let img = BootImage::new(kernel, dtb, BootImageVersion::V3);
        let packed = img.pack().unwrap();
        assert!(packed.len() > 0);
    }

    #[test]
    fn test_boot_image_with_cmdline() {
        let kernel = vec![0u8; 4096];
        let dtb = vec![0u8; 1024];
        let img = BootImage::new(kernel, dtb, BootImageVersion::V3)
            .with_cmdline("console=ttyMSM0,115200");
        let packed = img.pack().unwrap();
        assert!(packed.len() > 0);
    }
}
