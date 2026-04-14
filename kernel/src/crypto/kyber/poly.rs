//! Polynomial operations for Kyber
//! 
//! Provides polynomial arithmetic in both normal and NTT domains

#![cfg_attr(not(feature = "std"), no_std)]

use super::params::{KYBER_N, KYBER_Q};
use super::reduce::{barrett_reduce, freeze};
use super::ntt::{ntt, invntt, basemul};

/// Polynomial with 256 coefficients
#[derive(Clone, Copy)]
pub struct Poly {
    pub coeffs: [i16; KYBER_N],
}

impl Poly {
    /// Create zero polynomial
    #[inline]
    pub const fn zero() -> Self {
        Self { coeffs: [0; KYBER_N] }
    }
    
    /// Add two polynomials
    #[inline]
    pub fn add(&mut self, other: &Poly) {
        for i in 0..KYBER_N {
            self.coeffs[i] = self.coeffs[i].wrapping_add(other.coeffs[i]);
        }
    }
    
    /// Subtract two polynomials
    #[inline]
    pub fn sub(&mut self, other: &Poly) {
        for i in 0..KYBER_N {
            self.coeffs[i] = self.coeffs[i].wrapping_sub(other.coeffs[i]);
        }
    }
    
    /// Reduce all coefficients
    #[inline]
    pub fn reduce(&mut self) {
        for i in 0..KYBER_N {
            self.coeffs[i] = barrett_reduce(self.coeffs[i]);
        }
    }
    
    /// Normalize to [0, q-1]
    #[inline]
    pub fn normalize(&mut self) {
        for i in 0..KYBER_N {
            self.coeffs[i] = freeze(self.coeffs[i]);
        }
    }
    
    /// Transform to NTT domain
    #[inline]
    pub fn to_ntt(&mut self) {
        ntt(&mut self.coeffs);
    }
    
    /// Transform from NTT domain
    #[inline]
    pub fn from_ntt(&mut self) {
        invntt(&mut self.coeffs);
    }
    
    /// Pointwise multiply in NTT domain
    #[inline]
    pub fn pointwise_mul(&mut self, other: &Poly, zeta: i16) {
        basemul(&mut self.coeffs, &self.coeffs, &other.coeffs, zeta);
    }
    
    /// Encode to bytes (12 bits per coefficient)
    pub fn to_bytes(&self) -> [u8; 384] {
        let mut bytes = [0u8; 384];
        let mut t = [0u16; 8];
        
        for i in 0..KYBER_N / 8 {
            for j in 0..8 {
                t[j] = (self.coeffs[8 * i + j] as u16) & 0x0FFF;
            }
            
            bytes[12 * i] = t[0] as u8;
            bytes[12 * i + 1] = ((t[0] >> 8) | (t[1] << 4)) as u8;
            bytes[12 * i + 2] = (t[1] >> 4) as u8;
            bytes[12 * i + 3] = ((t[1] >> 12) | (t[2] << 8)) as u8;
            bytes[12 * i + 4] = (t[2] >> 0) as u8;
            bytes[12 * i + 5] = ((t[2] >> 8) | (t[3] << 4)) as u8;
            bytes[12 * i + 6] = (t[3] >> 4) as u8;
            bytes[12 * i + 7] = ((t[3] >> 12) | (t[4] << 8)) as u8;
            bytes[12 * i + 8] = (t[4] >> 0) as u8;
            bytes[12 * i + 9] = ((t[4] >> 8) | (t[5] << 4)) as u8;
            bytes[12 * i + 10] = (t[5] >> 4) as u8;
            bytes[12 * i + 11] = ((t[5] >> 12) | (t[6] << 8)) as u8;
        }
        
        bytes
    }
    
    /// Decode from bytes (12 bits per coefficient)
    pub fn from_bytes(bytes: &[u8; 384]) -> Self {
        let mut poly = Self::zero();
        
        for i in 0..KYBER_N / 8 {
            poly.coeffs[8 * i] = ((bytes[12 * i] as u16) | ((bytes[12 * i + 1] as u16) << 8)) as i16 & 0x0FFF;
            poly.coeffs[8 * i + 1] = (((bytes[12 * i + 1] as u16) >> 4) | ((bytes[12 * i + 2] as u16) << 4) | ((bytes[12 * i + 3] as u16) << 12)) as i16 & 0x0FFF;
            poly.coeffs[8 * i + 2] = (((bytes[12 * i + 3] as u16) >> 4) | ((bytes[12 * i + 4] as u16) << 4)) as i16 & 0x0FFF;
            poly.coeffs[8 * i + 3] = (((bytes[12 * i + 4] as u16) >> 8) | ((bytes[12 * i + 5] as u16) << 0) | ((bytes[12 * i + 6] as u16) << 8)) as i16 & 0x0FFF;
            poly.coeffs[8 * i + 4] = (((bytes[12 * i + 6] as u16) >> 4) | ((bytes[12 * i + 7] as u16) << 4) | ((bytes[12 * i + 8] as u16) << 12)) as i16 & 0x0FFF;
            poly.coeffs[8 * i + 5] = (((bytes[12 * i + 8] as u16) >> 4) | ((bytes[12 * i + 9] as u16) << 4)) as i16 & 0x0FFF;
            poly.coeffs[8 * i + 6] = (((bytes[12 * i + 9] as u16) >> 8) | ((bytes[12 * i + 10] as u16) << 0) | ((bytes[12 * i + 11] as u16) << 8)) as i16 & 0x0FFF;
            poly.coeffs[8 * i + 7] = ((bytes[12 * i + 11] as u16) >> 4) as i16 & 0x0FFF;
        }
        
        poly
    }
    
    /// Compress polynomial (d bits per coefficient)
    pub fn compress(&self, d: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mask = (1u16 << d) - 1;
        
        for i in 0..KYBER_N {
            let t = ((((self.coeffs[i] as u32) << d) + KYBER_Q as u32 / 2) / KYBER_Q as u32) as u16 & mask;
            
            // Pack bits
            if d == 4 {
                if i % 2 == 0 {
                    bytes.push(t as u8);
                } else {
                    let last = bytes.len() - 1;
                    bytes[last] |= (t as u8) << 4;
                }
            } else if d == 5 {
                // 5 bits packing
                let bit_pos = (i * d) % 8;
                let byte_pos = (i * d) / 8;
                
                while bytes.len() <= byte_pos + 1 {
                    bytes.push(0);
                }
                
                bytes[byte_pos] |= ((t << bit_pos) & 0xFF) as u8;
                if bit_pos + d > 8 {
                    bytes[byte_pos + 1] |= (t >> (8 - bit_pos)) as u8;
                }
            }
        }
        
        bytes
    }
    
    /// Decompress polynomial (d bits per coefficient)
    pub fn decompress(bytes: &[u8], d: usize) -> Self {
        let mut poly = Self::zero();
        let mask = (1u16 << d) - 1;
        
        for i in 0..KYBER_N {
            let t = if d == 4 {
                if i % 2 == 0 {
                    (bytes[i / 2] & 0x0F) as u16
                } else {
                    (bytes[i / 2] >> 4) as u16
                }
            } else if d == 5 {
                let bit_pos = (i * d) % 8;
                let byte_pos = (i * d) / 8;
                
                let mut t = ((bytes[byte_pos] as u16) >> bit_pos) & mask;
                if bit_pos + d > 8 && byte_pos + 1 < bytes.len() {
                    t |= ((bytes[byte_pos + 1] as u16) << (8 - bit_pos)) & mask;
                }
                t
            } else {
                0
            };
            
            poly.coeffs[i] = (((t as u32 * KYBER_Q as u32) + (1 << (d - 1))) >> d) as i16;
        }
        
        poly
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_poly_zero() {
        let p = Poly::zero();
        for &c in &p.coeffs {
            assert_eq!(c, 0);
        }
    }
    
    #[test]
    fn test_poly_add() {
        let mut a = Poly::zero();
        let mut b = Poly::zero();
        
        for i in 0..KYBER_N {
            a.coeffs[i] = i as i16;
            b.coeffs[i] = 1;
        }
        
        a.add(&b);
        
        for i in 0..KYBER_N {
            assert_eq!(a.coeffs[i], (i as i16) + 1);
        }
    }
    
    #[test]
    fn test_poly_sub() {
        let mut a = Poly::zero();
        let mut b = Poly::zero();
        
        for i in 0..KYBER_N {
            a.coeffs[i] = 100;
            b.coeffs[i] = 50;
        }
        
        a.sub(&b);
        
        for i in 0..KYBER_N {
            assert_eq!(a.coeffs[i], 50);
        }
    }
    
    #[test]
    fn test_poly_reduce() {
        let mut p = Poly::zero();
        for i in 0..KYBER_N {
            p.coeffs[i] = KYBER_Q + 100;
        }
        
        p.reduce();
        
        for &c in &p.coeffs {
            assert!(c >= 0 && c < KYBER_Q * 2);
        }
    }
    
    #[test]
    fn test_poly_normalize() {
        let mut p = Poly::zero();
        for i in 0..KYBER_N {
            p.coeffs[i] = KYBER_Q + 100;
        }
        
        p.normalize();
        
        for &c in &p.coeffs {
            assert!(c >= 0 && c < KYBER_Q);
        }
    }
    
    #[test]
    fn test_poly_encode_decode() {
        let mut p = Poly::zero();
        for i in 0..KYBER_N {
            p.coeffs[i] = (i % KYBER_Q as usize) as i16;
        }
        
        let bytes = p.to_bytes();
        let decoded = Poly::from_bytes(&bytes);
        
        for i in 0..KYBER_N {
            assert_eq!(p.coeffs[i] & 0x0FFF, decoded.coeffs[i]);
        }
    }
    
    #[test]
    fn test_poly_compress_decompress() {
        let mut p = Poly::zero();
        for i in 0..KYBER_N {
            p.coeffs[i] = ((i * 13) % KYBER_Q as usize) as i16;
        }
        p.normalize();
        
        let compressed = p.compress(4);
        let decompressed = Poly::decompress(&compressed, 4);
        
        // Should be approximately equal
        for i in 0..KYBER_N {
            let diff = (p.coeffs[i] - decompressed.coeffs[i]).abs();
            assert!(diff < KYBER_Q / 8);
        }
    }
}
