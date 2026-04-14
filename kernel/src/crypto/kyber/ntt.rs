//! Number Theoretic Transform (NTT) for Kyber
//! 
//! Cooley-Tukey forward NTT and Gentleman-Sande inverse NTT
//! Constant-time implementation

#![cfg_attr(not(feature = "std"), no_std)]

use super::params::{KYBER_N, ZETAS, ZETAS_INV};
use super::reduce::montgomery_reduce;

/// Forward NTT
/// 
/// Transforms polynomial from coefficient to NTT representation
/// Input: polynomial in normal order
/// Output: polynomial in bit-reversed order
#[inline]
pub fn ntt(poly: &mut [i16; KYBER_N]) {
    let mut k = 1;
    let mut len = 128;
    
    while len >= 2 {
        let mut start = 0;
        while start < 256 {
            let zeta = ZETAS[k];
            k += 1;
            
            for j in start..start + len {
                let t = montgomery_reduce(zeta as i32 * poly[j + len] as i32);
                poly[j + len] = poly[j] - t;
                poly[j] = poly[j] + t;
            }
            
            start += 2 * len;
        }
        
        len >>= 1;
    }
}

/// Inverse NTT
/// 
/// Transforms polynomial from NTT to coefficient representation
/// Input: polynomial in bit-reversed order
/// Output: polynomial in normal order
#[inline]
pub fn invntt(poly: &mut [i16; KYBER_N]) {
    let mut k = 0;
    let mut len = 2;
    
    while len <= 128 {
        let mut start = 0;
        while start < 256 {
            let zeta = ZETAS_INV[k];
            k += 1;
            
            for j in start..start + len {
                let t = poly[j];
                poly[j] = t + poly[j + len];
                poly[j + len] = t - poly[j + len];
                poly[j + len] = montgomery_reduce(zeta as i32 * poly[j + len] as i32);
            }
            
            start += 2 * len;
        }
        
        len <<= 1;
    }
    
    // Multiply by inverse of n
    for i in 0..256 {
        poly[i] = montgomery_reduce(poly[i] as i32 * 1441); // 1441 = n^(-1) * 2^16 mod q
    }
}

/// Multiply two polynomials in NTT domain (pointwise)
#[inline]
pub fn basemul(r: &mut [i16; KYBER_N], a: &[i16; KYBER_N], b: &[i16; KYBER_N], zeta: i16) {
    for i in (0..KYBER_N).step_by(4) {
        r[i] = montgomery_reduce(
            a[i + 1] as i32 * b[i + 1] as i32 * zeta as i32 +
            a[i] as i32 * b[i] as i32
        );
        r[i + 1] = montgomery_reduce(
            a[i] as i32 * b[i + 1] as i32 +
            a[i + 1] as i32 * b[i] as i32
        );
        r[i + 2] = montgomery_reduce(
            a[i + 3] as i32 * b[i + 3] as i32 * -zeta as i32 +
            a[i + 2] as i32 * b[i + 2] as i32
        );
        r[i + 3] = montgomery_reduce(
            a[i + 2] as i32 * b[i + 3] as i32 +
            a[i + 3] as i32 * b[i + 2] as i32
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::params::KYBER_Q;
    
    #[test]
    fn test_ntt_invntt_identity() {
        let mut poly = [0i16; 256];
        for i in 0..256 {
            poly[i] = (i as i16) % KYBER_Q;
        }
        
        let original = poly;
        
        ntt(&mut poly);
        invntt(&mut poly);
        
        // Should be close to original (modulo q)
        for i in 0..256 {
            let diff = (poly[i] - original[i]).abs();
            assert!(diff < 10, "Position {}: diff = {}", i, diff);
        }
    }
    
    #[test]
    fn test_ntt_zero() {
        let mut poly = [0i16; 256];
        ntt(&mut poly);
        
        for &coeff in &poly {
            assert_eq!(coeff, 0);
        }
    }
    
    #[test]
    fn test_invntt_zero() {
        let mut poly = [0i16; 256];
        invntt(&mut poly);
        
        for &coeff in &poly {
            assert_eq!(coeff, 0);
        }
    }
    
    #[test]
    fn test_basemul_zero() {
        let mut r = [0i16; 256];
        let a = [0i16; 256];
        let b = [1i16; 256];
        
        basemul(&mut r, &a, &b, 1);
        
        for &coeff in &r {
            assert_eq!(coeff, 0);
        }
    }
    
    #[test]
    fn test_ntt_range() {
        let mut poly = [100i16; 256];
        ntt(&mut poly);
        
        for &coeff in &poly {
            assert!(coeff.abs() < KYBER_Q * 2);
        }
    }
}
