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
#[inline(always)]
pub fn montgomery_reduce(a: i32) -> i16 {
    let t = (a as i64 * QINV as i64) as i16;
    let t = (a - t as i32 * KYBER_Q as i32) >> 16;
    t as i16
}

/// Barrett reduction
/// 
/// Computes a mod q
/// Input: -2^15 <= a < 2^15
/// Output: 0 <= r < q with r ≡ a (mod q)
#[inline(always)]
pub fn barrett_reduce(a: i16) -> i16 {
    let v = ((1 << 26) + KYBER_Q as i32 / 2) / KYBER_Q as i32;
    let t = (v * a as i32 + (1 << 25)) >> 26;
    let t = a - (t * KYBER_Q as i32) as i16;
    t
}

/// Conditional subtraction of q
/// 
/// Returns a - q if a >= q, else a
#[inline(always)]
pub fn csubq(a: i16) -> i16 {
    let a = a - KYBER_Q;
    let a = a + ((a >> 15) & KYBER_Q);
    a
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
