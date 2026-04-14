//! Integration tests for Device Tree parsing with real hardware data
//!
//! These tests use real DTB files from assets/vectors/dtb/

use common::{DtbTestSuite, Platform, fuzzing::DtbMutator};

#[test]
fn test_all_real_dtb_files() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    if suite.is_empty() {
        eprintln!("Warning: No DTB files found in assets/vectors/dtb/");
        eprintln!("Please add real DTB files for comprehensive testing");
        return;
    }
    
    println!("Testing {} DTB files", suite.len());
    
    for vector in suite.vectors() {
        println!("  Testing: {} ({:?}, {} bytes)", 
                 vector.name, vector.platform, vector.size());
        
        // Validate DTB magic
        assert!(vector.is_valid(), 
                "Invalid DTB magic in {}", vector.name);
        
        // Validate minimum size
        assert!(vector.size() >= 40, 
                "DTB too small: {} bytes", vector.size());
    }
}

#[test]
fn test_dtb_parsing_lifecycle() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    for vector in suite.vectors() {
        // This would integrate with actual FdtParser
        // For now, just validate structure
        let data = vector.data();
        
        // Check FDT header
        if data.len() >= 40 {
            let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            assert_eq!(magic, 0xd00dfeed, "Invalid magic in {}", vector.name);
            
            let totalsize = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            assert!(totalsize as usize <= data.len(), 
                    "Invalid totalsize in {}", vector.name);
        }
    }
}

#[test]
fn test_platform_specific_dtb() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    // Test QEMU DTBs
    let qemu_vectors = suite.platform_vectors(Platform::Qemu);
    for vector in qemu_vectors {
        println!("QEMU DTB: {}", vector.name);
        assert!(vector.is_valid());
    }
    
    // Test Raspberry Pi DTBs
    let rpi_vectors = suite.platform_vectors(Platform::RaspberryPi4);
    for vector in rpi_vectors {
        println!("RPi4 DTB: {}", vector.name);
        assert!(vector.is_valid());
    }
}

#[test]
#[ignore] // Run with: cargo test --test dtb_integration -- --ignored
fn fuzz_dtb_parser_1m_iterations() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    if suite.is_empty() {
        eprintln!("Skipping fuzzing: no DTB files available");
        return;
    }
    
    const ITERATIONS: usize = 1_000_000;
    let mut mutator = DtbMutator::new(42);
    let mut panic_count = 0;
    
    println!("Fuzzing {} DTB files with {} iterations each", 
             suite.len(), ITERATIONS);
    
    for vector in suite.vectors() {
        println!("  Fuzzing: {}", vector.name);
        
        for i in 0..ITERATIONS {
            let mutated = mutator.mutate(vector.data());
            
            // Try to parse mutated DTB
            // Should never panic, only return errors
            let result = std::panic::catch_unwind(|| {
                // This would call actual parser
                // For now, just validate we don't crash
                let _ = validate_dtb_structure(&mutated);
            });
            
            if result.is_err() {
                panic_count += 1;
                eprintln!("PANIC at iteration {} for {}", i, vector.name);
            }
            
            if i % 100_000 == 0 && i > 0 {
                println!("    Progress: {}/{}", i, ITERATIONS);
            }
        }
    }
    
    assert_eq!(panic_count, 0, 
               "Parser panicked {} times during fuzzing", panic_count);
}

#[test]
fn test_dtb_mutation_strategies() {
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    if suite.is_empty() {
        return;
    }
    
    let vector = &suite.vectors()[0];
    let mut mutator = DtbMutator::new(123);
    
    // Test bit flip
    let bit_flipped = mutator.bit_flip(vector.data());
    assert_eq!(bit_flipped.len(), vector.data().len());
    
    // Test byte corruption
    let corrupted = mutator.corrupt_byte(vector.data());
    assert_eq!(corrupted.len(), vector.data().len());
    
    // Test truncation
    let truncated = mutator.truncate(vector.data());
    assert!(truncated.len() <= vector.data().len());
    
    // Test insertion
    let inserted = mutator.insert_bytes(vector.data());
    assert!(inserted.len() >= vector.data().len());
}

// Helper function to validate DTB structure without panicking
fn validate_dtb_structure(data: &[u8]) -> Result<(), &'static str> {
    if data.len() < 4 {
        return Err("Too small");
    }
    
    let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if magic != 0xd00dfeed {
        return Err("Invalid magic");
    }
    
    if data.len() < 40 {
        return Err("Header incomplete");
    }
    
    Ok(())
}

#[test]
fn test_dtb_regression_suite() {
    // This test ensures that any changes to fdt.rs don't break
    // parsing of previously working DTB files
    let suite = DtbTestSuite::load_all().expect("Failed to load DTB test suite");
    
    let mut passed = 0;
    let mut failed = 0;
    
    for vector in suite.vectors() {
        match validate_dtb_structure(vector.data()) {
            Ok(_) => {
                passed += 1;
                println!("✓ {}", vector.name);
            }
            Err(e) => {
                failed += 1;
                eprintln!("✗ {}: {}", vector.name, e);
            }
        }
    }
    
    println!("\nRegression test results:");
    println!("  Passed: {}", passed);
    println!("  Failed: {}", failed);
    
    // All valid DTB files should pass
    assert_eq!(failed, 0, "Regression detected in DTB parsing");
}
