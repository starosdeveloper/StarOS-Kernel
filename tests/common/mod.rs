//! Global Test Infrastructure - Repository-Wide Host Testing
//!
//! This module provides the core testing infrastructure for the entire repository.
//! All tests run on the host architecture (x86_64) with real hardware data.
//!
//! Architecture:
//! - Dual-Target: aarch64-unknown-none (target) + host (testing)
//! - Data-Driven: Real DTB files from assets/vectors/
//! - Fuzzing: Property-based testing with real data mutation
//! - Coverage: Full lifecycle testing with sanitizers

use std::path::{Path, PathBuf};
use std::fs;

/// DTB test vector - real Device Tree Binary from hardware
#[derive(Debug, Clone)]
pub struct DtbVector {
    pub name: String,
    pub path: PathBuf,
    pub data: Vec<u8>,
    pub platform: Platform,
}

/// Hardware platform type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Qemu,
    RaspberryPi4,
    Pine64,
    Qualcomm,
    MediaTek,
    Exynos,
    Generic,
}

impl DtbVector {
    /// Load DTB from file
    pub fn load(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let path = path.as_ref();
        let data = fs::read(path)?;
        let name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        
        let platform = Self::detect_platform(&name);
        
        Ok(Self {
            name,
            path: path.to_path_buf(),
            data,
            platform,
        })
    }
    
    /// Detect platform from filename
    fn detect_platform(name: &str) -> Platform {
        let name_lower = name.to_lowercase();
        if name_lower.contains("qemu") {
            Platform::Qemu
        } else if name_lower.contains("rpi") || name_lower.contains("raspberry") {
            Platform::RaspberryPi4
        } else if name_lower.contains("pine64") {
            Platform::Pine64
        } else if name_lower.contains("qualcomm") || name_lower.contains("qcom") {
            Platform::Qualcomm
        } else if name_lower.contains("mediatek") || name_lower.contains("mtk") {
            Platform::MediaTek
        } else if name_lower.contains("exynos") {
            Platform::Exynos
        } else {
            Platform::Generic
        }
    }
    
    /// Get DTB data
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    
    /// Get DTB size
    pub fn size(&self) -> usize {
        self.data.len()
    }
    
    /// Validate DTB magic
    pub fn is_valid(&self) -> bool {
        if self.data.len() < 4 {
            return false;
        }
        let magic = u32::from_be_bytes([
            self.data[0],
            self.data[1],
            self.data[2],
            self.data[3],
        ]);
        magic == 0xd00dfeed
    }
}

/// DTB test suite - collection of all test vectors
pub struct DtbTestSuite {
    vectors: Vec<DtbVector>,
}

impl DtbTestSuite {
    /// Load all DTB files from assets directory
    pub fn load_all() -> Result<Self, std::io::Error> {
        let assets_dir = Self::find_assets_dir()?;
        let dtb_dir = assets_dir.join("vectors/dtb");
        
        let mut vectors = Vec::new();
        
        if dtb_dir.exists() {
            for entry in fs::read_dir(&dtb_dir)? {
                let entry = entry?;
                let path = entry.path();
                
                if path.extension().and_then(|s| s.to_str()) == Some("dtb") {
                    match DtbVector::load(&path) {
                        Ok(vector) => vectors.push(vector),
                        Err(e) => eprintln!("Warning: Failed to load {}: {}", path.display(), e),
                    }
                }
            }
        }
        
        Ok(Self { vectors })
    }
    
    /// Find assets directory (walk up from current dir)
    fn find_assets_dir() -> Result<PathBuf, std::io::Error> {
        let mut current = std::env::current_dir()?;
        
        loop {
            let assets = current.join("assets");
            if assets.exists() && assets.is_dir() {
                return Ok(assets);
            }
            
            if !current.pop() {
                break;
            }
        }
        
        // Fallback: create in current directory
        let assets = std::env::current_dir()?.join("assets");
        fs::create_dir_all(&assets)?;
        Ok(assets)
    }
    
    /// Get all vectors
    pub fn vectors(&self) -> &[DtbVector] {
        &self.vectors
    }
    
    /// Get vectors for specific platform
    pub fn platform_vectors(&self, platform: Platform) -> Vec<&DtbVector> {
        self.vectors.iter()
            .filter(|v| v.platform == platform)
            .collect()
    }
    
    /// Get vector by name
    pub fn get(&self, name: &str) -> Option<&DtbVector> {
        self.vectors.iter().find(|v| v.name == name)
    }
    
    /// Number of vectors
    pub fn len(&self) -> usize {
        self.vectors.len()
    }
    
    /// Is empty
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
}

/// Fuzzing utilities for mutation testing
pub mod fuzzing {
    use super::*;
    use rand::{Rng, SeedableRng};
    use rand::rngs::StdRng;
    
    /// Mutate DTB data for fuzzing
    pub struct DtbMutator {
        rng: StdRng,
    }
    
    impl DtbMutator {
        /// Create new mutator with seed
        pub fn new(seed: u64) -> Self {
            Self {
                rng: StdRng::seed_from_u64(seed),
            }
        }
        
        /// Bit-flip mutation
        pub fn bit_flip(&mut self, data: &[u8]) -> Vec<u8> {
            let mut mutated = data.to_vec();
            if mutated.is_empty() {
                return mutated;
            }
            
            let byte_idx = self.rng.gen_range(0..mutated.len());
            let bit_idx = self.rng.gen_range(0..8);
            mutated[byte_idx] ^= 1 << bit_idx;
            
            mutated
        }
        
        /// Byte corruption
        pub fn corrupt_byte(&mut self, data: &[u8]) -> Vec<u8> {
            let mut mutated = data.to_vec();
            if mutated.is_empty() {
                return mutated;
            }
            
            let idx = self.rng.gen_range(0..mutated.len());
            mutated[idx] = self.rng.gen();
            
            mutated
        }
        
        /// Truncate data
        pub fn truncate(&mut self, data: &[u8]) -> Vec<u8> {
            if data.len() < 2 {
                return data.to_vec();
            }
            
            let new_len = self.rng.gen_range(0..data.len());
            data[..new_len].to_vec()
        }
        
        /// Insert random bytes
        pub fn insert_bytes(&mut self, data: &[u8]) -> Vec<u8> {
            let mut mutated = data.to_vec();
            let insert_pos = self.rng.gen_range(0..=mutated.len());
            let insert_count = self.rng.gen_range(1..=16);
            
            for _ in 0..insert_count {
                mutated.insert(insert_pos, self.rng.gen());
            }
            
            mutated
        }
        
        /// Random mutation strategy
        pub fn mutate(&mut self, data: &[u8]) -> Vec<u8> {
            match self.rng.gen_range(0..4) {
                0 => self.bit_flip(data),
                1 => self.corrupt_byte(data),
                2 => self.truncate(data),
                _ => self.insert_bytes(data),
            }
        }
    }
}

/// Memory dump utilities for testing allocators
pub mod memory {
    /// Memory region for testing
    #[derive(Debug, Clone)]
    pub struct MemoryDump {
        pub base: usize,
        pub size: usize,
        pub data: Vec<u8>,
    }
    
    impl MemoryDump {
        /// Create new memory dump
        pub fn new(base: usize, size: usize) -> Self {
            Self {
                base,
                size,
                data: vec![0; size],
            }
        }
        
        /// Load from file
        pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, std::io::Error> {
            let data = std::fs::read(path)?;
            Ok(Self {
                base: 0,
                size: data.len(),
                data,
            })
        }
        
        /// Get data slice
        pub fn data(&self) -> &[u8] {
            &self.data
        }
    }
}

/// Hardware abstraction for host testing
pub mod hw_mock {
    /// Mock MMIO register
    pub struct MockRegister {
        value: u32,
    }
    
    impl MockRegister {
        pub fn new(initial: u32) -> Self {
            Self { value: initial }
        }
        
        pub fn read(&self) -> u32 {
            self.value
        }
        
        pub fn write(&mut self, val: u32) {
            self.value = val;
        }
    }
    
    /// Mock device for testing
    pub struct MockDevice {
        pub name: String,
        pub registers: Vec<MockRegister>,
    }
    
    impl MockDevice {
        pub fn new(name: impl Into<String>, num_regs: usize) -> Self {
            Self {
                name: name.into(),
                registers: vec![MockRegister::new(0); num_regs],
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_dtb_vector_platform_detection() {
        assert_eq!(DtbVector::detect_platform("qemu-virt"), Platform::Qemu);
        assert_eq!(DtbVector::detect_platform("rpi4"), Platform::RaspberryPi4);
        assert_eq!(DtbVector::detect_platform("qualcomm-sdm845"), Platform::Qualcomm);
    }
    
    #[test]
    fn test_dtb_mutator() {
        let mut mutator = fuzzing::DtbMutator::new(42);
        let original = vec![0xd0, 0x0d, 0xfe, 0xed];
        
        let mutated = mutator.bit_flip(&original);
        assert_eq!(mutated.len(), original.len());
        
        // Should have exactly one bit different
        let diff_bits: u32 = original.iter()
            .zip(mutated.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum();
        assert_eq!(diff_bits, 1);
    }
    
    #[test]
    fn test_memory_dump() {
        let dump = memory::MemoryDump::new(0x1000, 256);
        assert_eq!(dump.base, 0x1000);
        assert_eq!(dump.size, 256);
        assert_eq!(dump.data().len(), 256);
    }
}
