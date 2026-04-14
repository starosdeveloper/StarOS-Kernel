//! Sampling functions for Kyber
//! 
//! Uniform and CBD (Centered Binomial Distribution) sampling

#![cfg_attr(not(feature = "std"), no_std)]

use super::poly::Poly;
use super::params::{KYBER_N, KYBER_Q};
use super::shake::{Shake128, Shake256};

/// Sample polynomial uniformly from SHAKE-128
pub fn sample_uniform(seed: &[u8], nonce: u8) -> Poly {
    let mut shake = Shake128::new();
    shake.absorb(seed);
    shake.absorb(&[nonce]);
    shake.finalize();
    
    let mut poly = Poly::zero();
    let mut i = 0;
    let mut buf = [0u8; 3];
    
    while i < KYBER_N {
        shake.squeeze(&mut buf);
        
        let d1 = ((buf[0] as u16) | ((buf[1] as u16) << 8)) & 0x0FFF;
        let d2 = (((buf[1] as u16) >> 4) | ((buf[2] as u16) << 4)) & 0x0FFF;
        
        if d1 < KYBER_Q as u16 {
            poly.coeffs[i] = d1 as i16;
            i += 1;
        }
        
        if i < KYBER_N && d2 < KYBER_Q as u16 {
            poly.coeffs[i] = d2 as i16;
            i += 1;
        }
    }
    
    poly
}

/// Sample polynomial from centered binomial distribution
/// 
/// CBD_η: samples from {-η, ..., η} with binomial distribution
pub fn sample_cbd(buf: &[u8], eta: usize) -> Poly {
    let mut poly = Poly::zero();
    
    if eta == 2 {
        // CBD_2: 2 bits per coefficient
        for i in 0..KYBER_N {
            let byte_idx = i / 2;
            let bit_idx = (i % 2) * 4;
            
            let t = (buf[byte_idx] >> bit_idx) & 0x0F;
            let a = (t & 0x03).count_ones();
            let b = ((t >> 2) & 0x03).count_ones();
            
            poly.coeffs[i] = a as i16 - b as i16;
        }
    } else if eta == 3 {
        // CBD_3: 3 bits per coefficient
        let mut bit_pos = 0;
        for i in 0..KYBER_N {
            let byte_idx = bit_pos / 8;
            let shift = bit_pos % 8;
            
            let mut t = (buf[byte_idx] >> shift) as u16;
            if shift + 6 > 8 && byte_idx + 1 < buf.len() {
                t |= (buf[byte_idx + 1] as u16) << (8 - shift);
            }
            t &= 0x3F;
            
            let a = (t & 0x07).count_ones();
            let b = ((t >> 3) & 0x07).count_ones();
            
            poly.coeffs[i] = a as i16 - b as i16;
            bit_pos += 6;
        }
    }
    
    poly
}

/// Generate random bytes using SHAKE-256
pub fn prf(seed: &[u8], nonce: u8, output: &mut [u8]) {
    let mut shake = Shake256::new();
    shake.absorb(seed);
    shake.absorb(&[nonce]);
    shake.finalize();
    shake.squeeze(output);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sample_uniform() {
        let seed = [0u8; 32];
        let poly = sample_uniform(&seed, 0);
        
        // All coefficients should be in [0, q)
        for &c in &poly.coeffs {
            assert!(c >= 0 && c < KYBER_Q);
        }
    }
    
    #[test]
    fn test_sample_uniform_different_nonce() {
        let seed = [0u8; 32];
        let poly1 = sample_uniform(&seed, 0);
        let poly2 = sample_uniform(&seed, 1);
        
        // Different nonces should give different polynomials
        assert_ne!(poly1.coeffs, poly2.coeffs);
    }
    
    #[test]
    fn test_sample_cbd2() {
        let buf = [0x55u8; 128]; // Alternating bits
        let poly = sample_cbd(&buf, 2);
        
        // All coefficients should be in [-2, 2]
        for &c in &poly.coeffs {
            assert!(c >= -2 && c <= 2);
        }
    }
    
    #[test]
    fn test_sample_cbd3() {
        let buf = [0xFFu8; 192];
        let poly = sample_cbd(&buf, 3);
        
        // All coefficients should be in [-3, 3]
        for &c in &poly.coeffs {
            assert!(c >= -3 && c <= 3);
        }
    }
    
    #[test]
    fn test_prf() {
        let seed = [0u8; 32];
        let mut out1 = [0u8; 64];
        let mut out2 = [0u8; 64];
        
        prf(&seed, 0, &mut out1);
        prf(&seed, 1, &mut out2);
        
        // Different nonces should give different outputs
        assert_ne!(out1, out2);
    }
}
