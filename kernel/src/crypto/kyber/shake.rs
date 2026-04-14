//! SHAKE-128 and SHAKE-256 (Keccak-based XOF)
//! 
//! Extendable-Output Functions for Kyber sampling

#![cfg_attr(not(feature = "std"), no_std)]

const KECCAK_ROUNDS: usize = 24;

/// Keccak round constants
const RC: [u64; 24] = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808a, 0x8000000080008000,
    0x000000000000808b, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008a, 0x0000000000000088, 0x0000000080008009, 0x000000008000000a,
    0x000000008000808b, 0x800000000000008b, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800a, 0x800000008000000a,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
];

/// Keccak-f[1600] permutation
#[inline]
fn keccak_f1600(state: &mut [u64; 25]) {
    for round in 0..KECCAK_ROUNDS {
        // θ (theta)
        let mut c = [0u64; 5];
        for x in 0..5 {
            c[x] = state[x] ^ state[x + 5] ^ state[x + 10] ^ state[x + 15] ^ state[x + 20];
        }
        
        for x in 0..5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            for y in 0..5 {
                state[x + 5 * y] ^= d;
            }
        }
        
        // ρ (rho) and π (pi)
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                b[y + 5 * ((2 * x + 3 * y) % 5)] = state[x + 5 * y].rotate_left(
                    [0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14][x + 5 * y]
                );
            }
        }
        
        // χ (chi)
        for y in 0..5 {
            let t = [b[5 * y], b[5 * y + 1], b[5 * y + 2], b[5 * y + 3], b[5 * y + 4]];
            for x in 0..5 {
                state[x + 5 * y] = t[x] ^ ((!t[(x + 1) % 5]) & t[(x + 2) % 5]);
            }
        }
        
        // ι (iota)
        state[0] ^= RC[round];
    }
}

/// SHAKE-128 context
pub struct Shake128 {
    state: [u64; 25],
    pos: usize,
    rate: usize,
}

impl Shake128 {
    /// Create new SHAKE-128 instance
    #[inline]
    pub fn new() -> Self {
        Self {
            state: [0u64; 25],
            pos: 0,
            rate: 168, // 1344 bits = 168 bytes
        }
    }
    
    /// Absorb input data
    pub fn absorb(&mut self, input: &[u8]) {
        for &byte in input {
            let idx = self.pos / 8;
            let shift = (self.pos % 8) * 8;
            self.state[idx] ^= (byte as u64) << shift;
            self.pos += 1;
            
            if self.pos == self.rate {
                keccak_f1600(&mut self.state);
                self.pos = 0;
            }
        }
    }
    
    /// Finalize absorption (padding)
    pub fn finalize(&mut self) {
        // Pad with 0x1F (SHAKE-128 domain separator)
        let idx = self.pos / 8;
        let shift = (self.pos % 8) * 8;
        self.state[idx] ^= 0x1F << shift;
        
        // Pad with 0x80 at end of rate
        let last_idx = (self.rate - 1) / 8;
        let last_shift = ((self.rate - 1) % 8) * 8;
        self.state[last_idx] ^= 0x80 << last_shift;
        
        keccak_f1600(&mut self.state);
        self.pos = 0;
    }
    
    /// Squeeze output data
    pub fn squeeze(&mut self, output: &mut [u8]) {
        for byte in output.iter_mut() {
            if self.pos == self.rate {
                keccak_f1600(&mut self.state);
                self.pos = 0;
            }
            
            let idx = self.pos / 8;
            let shift = (self.pos % 8) * 8;
            *byte = (self.state[idx] >> shift) as u8;
            self.pos += 1;
        }
    }
}

/// SHAKE-256 context
pub struct Shake256 {
    state: [u64; 25],
    pos: usize,
    rate: usize,
}

impl Shake256 {
    /// Create new SHAKE-256 instance
    #[inline]
    pub fn new() -> Self {
        Self {
            state: [0u64; 25],
            pos: 0,
            rate: 136, // 1088 bits = 136 bytes
        }
    }
    
    /// Absorb input data
    pub fn absorb(&mut self, input: &[u8]) {
        for &byte in input {
            let idx = self.pos / 8;
            let shift = (self.pos % 8) * 8;
            self.state[idx] ^= (byte as u64) << shift;
            self.pos += 1;
            
            if self.pos == self.rate {
                keccak_f1600(&mut self.state);
                self.pos = 0;
            }
        }
    }
    
    /// Finalize absorption (padding)
    pub fn finalize(&mut self) {
        // Pad with 0x1F (SHAKE-256 domain separator)
        let idx = self.pos / 8;
        let shift = (self.pos % 8) * 8;
        self.state[idx] ^= 0x1F << shift;
        
        // Pad with 0x80 at end of rate
        let last_idx = (self.rate - 1) / 8;
        let last_shift = ((self.rate - 1) % 8) * 8;
        self.state[last_idx] ^= 0x80 << last_shift;
        
        keccak_f1600(&mut self.state);
        self.pos = 0;
    }
    
    /// Squeeze output data
    pub fn squeeze(&mut self, output: &mut [u8]) {
        for byte in output.iter_mut() {
            if self.pos == self.rate {
                keccak_f1600(&mut self.state);
                self.pos = 0;
            }
            
            let idx = self.pos / 8;
            let shift = (self.pos % 8) * 8;
            *byte = (self.state[idx] >> shift) as u8;
            self.pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_shake128_empty() {
        let mut shake = Shake128::new();
        shake.finalize();
        
        let mut output = [0u8; 32];
        shake.squeeze(&mut output);
        
        // Known answer for empty input
        let expected = [
            0x7f, 0x9c, 0x2b, 0xa4, 0xe8, 0x8f, 0x82, 0x7d,
            0x61, 0x60, 0x45, 0x50, 0x76, 0x05, 0x85, 0x3e,
            0xd7, 0x3b, 0x80, 0x93, 0xf6, 0xef, 0xbc, 0x88,
            0xeb, 0x1a, 0x6e, 0xac, 0xfa, 0x66, 0xef, 0x26,
        ];
        
        assert_eq!(output, expected);
    }
    
    #[test]
    fn test_shake256_empty() {
        let mut shake = Shake256::new();
        shake.finalize();
        
        let mut output = [0u8; 32];
        shake.squeeze(&mut output);
        
        // Known answer for empty input
        let expected = [
            0x46, 0xb9, 0xdd, 0x2b, 0x0b, 0xa8, 0x8d, 0x13,
            0x23, 0x3b, 0x3f, 0xeb, 0x74, 0x3e, 0xeb, 0x24,
            0x3f, 0xcd, 0x52, 0xea, 0x62, 0xb8, 0x1b, 0x82,
            0xb5, 0x0c, 0x27, 0x64, 0x6e, 0xd5, 0x76, 0x2f,
        ];
        
        assert_eq!(output, expected);
    }
    
    #[test]
    fn test_shake128_abc() {
        let mut shake = Shake128::new();
        shake.absorb(b"abc");
        shake.finalize();
        
        let mut output = [0u8; 16];
        shake.squeeze(&mut output);
        
        // Should produce deterministic output
        assert_ne!(output, [0u8; 16]);
    }
}
