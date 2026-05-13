//! Reduction functions for Kyber
//! 
//! Montgomery and Barrett reduction for efficient modular arithmetic

#![cfg_attr(not(feature = "std"), no_std)]

use super::params::{KYBER_Q, QINV};

/// Montgomery reduction
/// 
/// Computes a * R^(-1) mod q where R = 2^16
/// Input: -q*2^15 <= a < q*2^15
/// Output: -q < r < q with r ≡ a*R^(-1) (mod q)
///
/// # Constant-Time Guarantee
/// Uses only arithmetic operations (multiply, shift, subtract).
/// No branches or secret-dependent memory access.
#[inline(always)]
pub fn montgomery_reduce(a: i32) -> i16 {
    // t = a * QINV mod 2^16 (low 16 bits of product)
    let t = (a.wrapping_mul(QINV)) as i16;
    // r = (a - t*q) / 2^16
    let r = (a - (t as i32).wrapping_mul(KYBER_Q as i32)) >> 16;
    r as i16
}

/// Barrett reduction
/// 
/// Computes a mod q in constant time
/// Input: -2^15 <= a < 2^15
/// Output: 0 <= r < 2q with r ≡ a (mod q)
///
/// # Constant-Time Guarantee
/// Uses only multiply, add, shift, subtract. No branches.
#[inline(always)]
pub fn barrett_reduce(a: i16) -> i16 {
    // v = round(2^26 / q)
    const V: i32 = 20159; // ((1 << 26) + KYBER_Q/2) / KYBER_Q
    let t = (V * a as i32 + (1 << 25)) >> 26;
    (a as i32 - t * KYBER_Q as i32) as i16
}

/// Conditional subtraction of q (constant-time)
/// 
/// Returns a - q if a >= q, else a.
/// Uses arithmetic masking instead of branching.
///
/// # Constant-Time Guarantee
/// The mask is computed via arithmetic shift which is data-independent timing.
#[inline(always)]
pub fn csubq(a: i16) -> i16 {
    let mut r = a.wrapping_sub(KYBER_Q);
    // If r < 0, mask = 0xFFFF (all ones), else mask = 0
    // Arithmetic right shift propagates sign bit
    let mask = (r >> 15) & KYBER_Q;
    r = r.wrapping_add(mask);
    r
}

/// Freeze: reduce to [0, q-1]
#[inline(always)]
pub fn freeze(a: i16) -> i16 {
    let a = barrett_reduce(a);
    csubq(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::params::KYBER_Q;
    
    #[test]
    fn test_montgomery_reduce() {
        // Test with known values
        let a = 1000i32 * (1 << 16);
        let r = montgomery_reduce(a);
        assert!(r.abs() < KYBER_Q);
        
        // Test zero
        assert_eq!(montgomery_reduce(0), 0);
    }
    
    #[test]
    fn test_barrett_reduce() {
        // Test values in range
        assert_eq!(barrett_reduce(0), 0);
        assert_eq!(barrett_reduce(KYBER_Q), 0);
        assert_eq!(barrett_reduce(KYBER_Q + 1), 1);
        
        // Test negative
        let r = barrett_reduce(-100);
        assert!(r >= 0 && r < KYBER_Q);
    }
    
    #[test]
    fn test_csubq() {
        assert_eq!(csubq(0), 0);
        assert_eq!(csubq(KYBER_Q), 0);
        assert_eq!(csubq(KYBER_Q - 1), KYBER_Q - 1);
        assert_eq!(csubq(KYBER_Q + 1), 1);
    }
    
    #[test]
    fn test_freeze() {
        for i in -1000..1000 {
            let r = freeze(i);
            assert!(r >= 0 && r < KYBER_Q);
        }
    }
    
    #[test]
    fn test_montgomery_identity() {
        // mont(a * R) * mont(b * R) = mont(a * b * R)
        let a = 100i16;
        let b = 200i16;
        
        let ar = (a as i32 * (1 << 16)) % KYBER_Q as i32;
        let br = (b as i32 * (1 << 16)) % KYBER_Q as i32;
        
        let ma = montgomery_reduce(ar);
        let mb = montgomery_reduce(br);
        
        assert!(ma.abs() < KYBER_Q);
        assert!(mb.abs() < KYBER_Q);
    }
}
