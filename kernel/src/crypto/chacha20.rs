//! ChaCha20 stream cipher for CSPRNG
//! 
//! RFC 8439 compliant implementation
//! 
//! # Security Guarantees
//! 
//! - **Constant-time operations:** All operations use constant-time primitives
//!   (wrapping_add, rotate_left) that do not branch on secret data
//! - **No secret-dependent branches:** Control flow is independent of key/nonce
//! - **No secret-dependent memory access:** All array accesses use fixed indices
//! - **Zeroization:** Sensitive data is zeroed on drop (TODO: implement Drop)
//! 
//! # Side-Channel Resistance
//! 
//! This implementation is designed to resist:
//! - Timing attacks: No variable-time operations on secrets
//! - Cache attacks: No secret-dependent memory access patterns
//! 
//! # Formal Verification Status
//! 
//! - ✅ RFC 8439 test vectors passing
//! - ⚠️ Constant-time properties: Verified by inspection, needs formal proof
//! - ⚠️ Side-channel resistance: Needs hardware-level testing

#![cfg_attr(not(feature = "std"), no_std)]

/// ChaCha20 constants "expand 32-byte k"
const CONSTANTS: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

/// ChaCha20 quarter round - constant time
/// 
/// # Constant-Time Guarantee
/// 
/// Uses only constant-time operations:
/// - `wrapping_add`: Constant-time addition (no carry flag inspection)
/// - `rotate_left`: Constant-time rotation (single instruction on ARM64)
/// - XOR (`^`): Constant-time bitwise operation
/// 
/// No branches, no secret-dependent memory access.
#[inline(always)]
fn quarter_round(a: &mut u32, b: &mut u32, c: &mut u32, d: &mut u32) {
    *a = a.wrapping_add(*b); *d ^= *a; *d = d.rotate_left(16);
    *c = c.wrapping_add(*d); *b ^= *c; *b = b.rotate_left(12);
    *a = a.wrapping_add(*b); *d ^= *a; *d = d.rotate_left(8);
    *c = c.wrapping_add(*d); *b ^= *c; *b = b.rotate_left(7);
}

/// ChaCha20 block function
/// 
/// # Arguments
/// * `key` - 256-bit key
/// * `nonce` - 96-bit nonce
/// * `counter` - 32-bit block counter
/// 
/// # Returns
/// 64-byte keystream block
/// 
/// # Constant-Time Guarantee
/// 
/// This function executes in constant time regardless of input values:
/// - Fixed number of iterations (10 double rounds = 20 rounds)
/// - No conditional branches on secret data
/// - All array accesses use compile-time constant indices
/// - All arithmetic operations are constant-time (wrapping_add, rotate_left, XOR)
/// 
/// # Security Assertions
/// 
/// - Key and nonce are never used in branch conditions
/// - No variable-length loops dependent on secrets
/// - Memory access pattern is independent of key/nonce/counter
#[inline]
pub fn chacha20_block(key: &[u8; 32], nonce: &[u8; 12], counter: u32) -> [u8; 64] {
    let mut state = [0u32; 16];
    
    // Initialize state
    state[0..4].copy_from_slice(&CONSTANTS);
    
    // Key (8 words)
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([
            key[i * 4], key[i * 4 + 1], key[i * 4 + 2], key[i * 4 + 3]
        ]);
    }
    
    // Counter (1 word)
    state[12] = counter;
    
    // Nonce (3 words)
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[i * 4], nonce[i * 4 + 1], nonce[i * 4 + 2], nonce[i * 4 + 3]
        ]);
    }
    
    let mut working = state;
    
    // 20 rounds (10 double rounds)
    for _ in 0..10 {
        // Column rounds
        quarter_round(&mut working[0], &mut working[4], &mut working[8], &mut working[12]);
        quarter_round(&mut working[1], &mut working[5], &mut working[9], &mut working[13]);
        quarter_round(&mut working[2], &mut working[6], &mut working[10], &mut working[14]);
        quarter_round(&mut working[3], &mut working[7], &mut working[11], &mut working[15]);
        
        // Diagonal rounds
        quarter_round(&mut working[0], &mut working[5], &mut working[10], &mut working[15]);
        quarter_round(&mut working[1], &mut working[6], &mut working[11], &mut working[12]);
        quarter_round(&mut working[2], &mut working[7], &mut working[8], &mut working[13]);
        quarter_round(&mut working[3], &mut working[4], &mut working[9], &mut working[14]);
    }
    
    // Add original state (constant time)
    for i in 0..16 {
        working[i] = working[i].wrapping_add(state[i]);
    }
    
    // Serialize to bytes (little-endian)
    let mut output = [0u8; 64];
    for i in 0..16 {
        output[i * 4..(i + 1) * 4].copy_from_slice(&working[i].to_le_bytes());
    }
    
    // Zeroize sensitive intermediate state
    // This prevents key material from lingering in memory
    // Note: Compiler may optimize this away - use volatile writes in production
    for i in 0..16 {
        state[i] = 0;
        working[i] = 0;
    }
    
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_quarter_round_rfc8439() {
        let mut a = 0x11111111;
        let mut b = 0x01020304;
        let mut c = 0x9b8d6f43;
        let mut d = 0x01234567;
        
        quarter_round(&mut a, &mut b, &mut c, &mut d);
        
        assert_eq!(a, 0xea2a92f4);
        assert_eq!(b, 0xcb1cf8ce);
        assert_eq!(c, 0x4581472e);
        assert_eq!(d, 0x5881c4bb);
    }
    
    #[test]
    fn test_chacha20_block_rfc8439() {
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let nonce = [0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00, 0x00];
        let counter = 1;
        
        let output = chacha20_block(&key, &nonce, counter);
        
        let expected = [
            0x10, 0xf1, 0xe7, 0xe4, 0xd1, 0x3b, 0x59, 0x15,
            0x50, 0x0f, 0xdd, 0x1f, 0xa3, 0x20, 0x71, 0xc4,
            0xc7, 0xd1, 0xf4, 0xc7, 0x33, 0xc0, 0x68, 0x03,
            0x04, 0x22, 0xaa, 0x9a, 0xc3, 0xd4, 0x6c, 0x4e,
            0xd2, 0x82, 0x64, 0x46, 0x07, 0x9f, 0xaa, 0x09,
            0x14, 0xc2, 0xd7, 0x05, 0xd9, 0x8b, 0x02, 0xa2,
            0xb5, 0x12, 0x9c, 0xd1, 0xde, 0x16, 0x4e, 0xb9,
            0xcb, 0xd0, 0x83, 0xe8, 0xa2, 0x50, 0x3c, 0x4e,
        ];
        
        assert_eq!(&output[..], &expected[..]);
    }
    
    #[test]
    fn test_chacha20_zero_key() {
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let output = chacha20_block(&key, &nonce, 0);
        
        // Should produce non-zero output even with zero key
        assert_ne!(output, [0u8; 64]);
    }
    
    #[test]
    fn test_chacha20_different_counters() {
        let key = [1u8; 32];
        let nonce = [2u8; 12];
        
        let out1 = chacha20_block(&key, &nonce, 0);
        let out2 = chacha20_block(&key, &nonce, 1);
        
        assert_ne!(out1, out2);
    }
}
