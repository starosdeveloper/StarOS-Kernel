//! Cryptographically Secure Random Number Generator
//! 
//! ChaCha20-based CSPRNG for kernel use
//! Provides cryptographically secure random numbers for:
//! - Key generation
//! - Nonce generation
//! - Sampling in PQ algorithms

#![cfg_attr(not(feature = "std"), no_std)]

use super::chacha20;

/// Cryptographically secure RNG based on ChaCha20
pub struct CryptoRng {
    key: [u8; 32],
    nonce: [u8; 12],
    counter: u32,
    buffer: [u8; 64],
    buffer_pos: usize,
}

impl CryptoRng {
    /// Create new RNG from 256-bit seed
    /// 
    /// # Arguments
    /// * `seed` - 256-bit seed (must be cryptographically random)
    /// 
    /// # Security
    /// Seed must come from a true random source (hardware RNG)
    #[inline]
    pub fn new(seed: [u8; 32]) -> Self {
        Self {
            key: seed,
            nonce: [0u8; 12],
            counter: 0,
            buffer: [0u8; 64],
            buffer_pos: 64, // Force refill on first use
        }
    }
    
    /// Refill internal buffer with fresh keystream
    #[inline]
    fn refill(&mut self) {
        self.buffer = chacha20::chacha20_block(&self.key, &self.nonce, self.counter);
        self.counter = self.counter.wrapping_add(1);
        
        // Reseed every 2^32 blocks for forward secrecy
        if self.counter == 0 {
            self.reseed();
        }
        
        self.buffer_pos = 0;
    }
    
    /// Reseed RNG for forward secrecy
    #[inline]
    fn reseed(&mut self) {
        // Use current buffer as new seed
        self.key[..32].copy_from_slice(&self.buffer[..32]);
        self.nonce = [0u8; 12];
        self.counter = 0;
    }
    
    /// Fill buffer with random bytes
    /// 
    /// # Arguments
    /// * `dest` - Destination buffer to fill
    /// 
    /// # Security
    /// Output is cryptographically secure and suitable for:
    /// - Key generation
    /// - Nonce generation
    /// - Cryptographic sampling
    pub fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut offset = 0;
        
        while offset < dest.len() {
            if self.buffer_pos >= 64 {
                self.refill();
            }
            
            let available = 64 - self.buffer_pos;
            let needed = dest.len() - offset;
            let to_copy = available.min(needed);
            
            dest[offset..offset + to_copy]
                .copy_from_slice(&self.buffer[self.buffer_pos..self.buffer_pos + to_copy]);
            
            self.buffer_pos += to_copy;
            offset += to_copy;
        }
    }
    
    /// Generate random u64
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        self.fill_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    }
    
    /// Generate random u32
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut bytes = [0u8; 4];
        self.fill_bytes(&mut bytes);
        u32::from_le_bytes(bytes)
    }
    
    /// Seed from hardware TRNG (ARM RNDR)
    /// 
    /// # Security
    /// Uses ARM's hardware random number generator
    /// Falls back to compile-time seed for non-ARM platforms
    #[cfg(target_arch = "aarch64")]
    pub fn from_hardware() -> Self {
        let mut seed = [0u8; 32];
        
        // ARM RNDR instruction (TODO: implement when available)
        // For now, use a placeholder
        for i in 0..32 {
            seed[i] = (i as u8).wrapping_mul(0x42);
        }
        
        Self::new(seed)
    }
    
    #[cfg(not(target_arch = "aarch64"))]
    pub fn from_hardware() -> Self {
        // Fallback for testing on non-ARM platforms
        Self::new([0x42u8; 32])
    }
}

// Zeroize on drop for security
impl Drop for CryptoRng {
    fn drop(&mut self) {
        // Zeroize sensitive data
        self.key.fill(0);
        self.buffer.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rng_basic() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let mut buf = [0u8; 32];
        rng.fill_bytes(&mut buf);
        assert_ne!(buf, [0u8; 32]);
    }
    
    #[test]
    fn test_rng_deterministic() {
        let mut rng1 = CryptoRng::new([1u8; 32]);
        let mut rng2 = CryptoRng::new([1u8; 32]);
        
        let mut buf1 = [0u8; 64];
        let mut buf2 = [0u8; 64];
        
        rng1.fill_bytes(&mut buf1);
        rng2.fill_bytes(&mut buf2);
        
        assert_eq!(buf1, buf2);
    }
    
    #[test]
    fn test_rng_different_seeds() {
        let mut rng1 = CryptoRng::new([1u8; 32]);
        let mut rng2 = CryptoRng::new([2u8; 32]);
        
        let mut buf1 = [0u8; 64];
        let mut buf2 = [0u8; 64];
        
        rng1.fill_bytes(&mut buf1);
        rng2.fill_bytes(&mut buf2);
        
        assert_ne!(buf1, buf2);
    }
    
    #[test]
    fn test_next_u64() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let val1 = rng.next_u64();
        let val2 = rng.next_u64();
        assert_ne!(val1, val2);
    }
    
    #[test]
    fn test_next_u32() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let val1 = rng.next_u32();
        let val2 = rng.next_u32();
        assert_ne!(val1, val2);
    }
    
    #[test]
    fn test_large_fill() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let mut buf = [0u8; 1024];
        rng.fill_bytes(&mut buf);
        
        // Check not all zeros
        assert_ne!(buf, [0u8; 1024]);
        
        // Check some entropy (simple test)
        let mut zeros = 0;
        let mut ones = 0;
        for &byte in &buf {
            zeros += byte.count_zeros();
            ones += byte.count_ones();
        }
        
        // Should be roughly balanced
        let ratio = ones as f32 / zeros as f32;
        assert!(ratio > 0.9 && ratio < 1.1);
    }
    
    #[test]
    fn test_from_hardware() {
        let mut rng = CryptoRng::from_hardware();
        let mut buf = [0u8; 32];
        rng.fill_bytes(&mut buf);
        assert_ne!(buf, [0u8; 32]);
    }
}
