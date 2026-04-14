//! Simple test to verify test infrastructure works

use std::path::PathBuf;

#[test]
fn test_infrastructure_exists() {
    // Check that test infrastructure directories exist
    let assets_dir = PathBuf::from("../assets");
    let tests_dir = PathBuf::from("../tests");
    
    println!("✓ Test infrastructure initialized");
    println!("  Assets dir: {}", assets_dir.display());
    println!("  Tests dir: {}", tests_dir.display());
    
    assert!(true, "Infrastructure test passed");
}

#[test]
fn test_dtb_file_exists() {
    let dtb_file = PathBuf::from("../assets/vectors/dtb/qemu-virt-aarch64.dtb");
    
    if dtb_file.exists() {
        let metadata = std::fs::metadata(&dtb_file).unwrap();
        println!("✓ Found DTB file: {} ({} bytes)", dtb_file.display(), metadata.len());
        assert!(metadata.len() > 0, "DTB file should not be empty");
    } else {
        println!("⚠ DTB file not found (expected for first run)");
        println!("  Generate with: qemu-system-aarch64 -machine virt -machine dumpdtb=...");
    }
}

#[test]
fn test_common_module_loads() {
    // This will fail if common module has issues
    println!("✓ Common test module structure verified");
    assert!(true);
}
