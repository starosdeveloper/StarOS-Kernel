//! Kyber768 Key Generation
//! 
//! Generates public and secret keypairs for Kyber768

#![cfg_attr(not(feature = "std"), no_std)]

use crate::crypto::rng::CryptoRng;
use super::poly::Poly;
use super::params::{KYBER_K, KYBER_ETA, KYBER_N};
use super::sampling::{sample_uniform, sample_cbd, prf};
use super::ntt::ntt;

/// Public key for Kyber768
#[derive(Clone)]
pub struct PublicKey {
    /// Polynomial vector t = A*s + e
    pub t: [Poly; KYBER_K],
    /// Seed for matrix A
    pub rho: [u8; 32],
}

/// Secret key for Kyber768
#[derive(Clone)]
pub struct SecretKey {
    /// Secret polynomial vector s
    pub s: [Poly; KYBER_K],
}

impl PublicKey {
    /// Encode public key to bytes (1184 bytes for Kyber768)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1184);
        
        // Encode t (3 * 384 = 1152 bytes)
        for poly in &self.t {
            bytes.extend_from_slice(&poly.to_bytes());
        }
        
        // Append rho (32 bytes)
        bytes.extend_from_slice(&self.rho);
        
        bytes
    }
    
    /// Decode public key from bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut t = [Poly::zero(); KYBER_K];
        
        // Decode t
        for i in 0..KYBER_K {
            let start = i * 384;
            let mut poly_bytes = [0u8; 384];
            poly_bytes.copy_from_slice(&bytes[start..start + 384]);
            t[i] = Poly::from_bytes(&poly_bytes);
        }
        
        // Decode rho
        let mut rho = [0u8; 32];
        rho.copy_from_slice(&bytes[1152..1184]);
        
        Self { t, rho }
    }
}

impl SecretKey {
    /// Encode secret key to bytes (1152 bytes for Kyber768)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1152);
        
        // Encode s (3 * 384 = 1152 bytes)
        for poly in &self.s {
            bytes.extend_from_slice(&poly.to_bytes());
        }
        
        bytes
    }
    
    /// Decode secret key from bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut s = [Poly::zero(); KYBER_K];
        
        // Decode s
        for i in 0..KYBER_K {
            let start = i * 384;
            let mut poly_bytes = [0u8; 384];
            poly_bytes.copy_from_slice(&bytes[start..start + 384]);
            s[i] = Poly::from_bytes(&poly_bytes);
        }
        
        Self { s }
    }
}

/// Generate Kyber768 keypair
/// 
/// Returns (public_key, secret_key)
pub fn keypair(rng: &mut CryptoRng) -> (PublicKey, SecretKey) {
    // Generate random seeds
    let mut seed = [0u8; 64];
    rng.fill_bytes(&mut seed);
    
    let mut rho = [0u8; 32];
    let mut sigma = [0u8; 32];
    rho.copy_from_slice(&seed[0..32]);
    sigma.copy_from_slice(&seed[32..64]);
    
    // Generate matrix A from rho
    let mut a = [[Poly::zero(); KYBER_K]; KYBER_K];
    for i in 0..KYBER_K {
        for j in 0..KYBER_K {
            a[i][j] = sample_uniform(&rho, (i * KYBER_K + j) as u8);
        }
    }
    
    // Sample secret vector s
    let mut s = [Poly::zero(); KYBER_K];
    for i in 0..KYBER_K {
        let mut noise_buf = [0u8; KYBER_ETA * KYBER_N / 4];
        prf(&sigma, i as u8, &mut noise_buf);
        s[i] = sample_cbd(&noise_buf, KYBER_ETA);
        s[i].to_ntt();
    }
    
    // Sample error vector e
    let mut e = [Poly::zero(); KYBER_K];
    for i in 0..KYBER_K {
        let mut noise_buf = [0u8; KYBER_ETA * KYBER_N / 4];
        prf(&sigma, (KYBER_K + i) as u8, &mut noise_buf);
        e[i] = sample_cbd(&noise_buf, KYBER_ETA);
        e[i].to_ntt();
    }
    
    // Compute t = A*s + e
    let mut t = [Poly::zero(); KYBER_K];
    for i in 0..KYBER_K {
        t[i] = Poly::zero();
        for j in 0..KYBER_K {
            let mut temp = a[i][j].clone();
            temp.pointwise_mul(&s[j], 1);
            t[i].add(&temp);
        }
        t[i].add(&e[i]);
        t[i].reduce();
    }
    
    // Convert s back from NTT for storage
    let mut s_normal = s.clone();
    for poly in &mut s_normal {
        poly.from_ntt();
        poly.normalize();
    }
    
    let pk = PublicKey { t, rho };
    let sk = SecretKey { s: s_normal };
    
    (pk, sk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::rng::CryptoRng;
    
    #[test]
    fn test_keypair_generation() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let (pk, sk) = keypair(&mut rng);
        
        // Check dimensions
        assert_eq!(pk.t.len(), KYBER_K);
        assert_eq!(sk.s.len(), KYBER_K);
        assert_eq!(pk.rho.len(), 32);
    }
    
    #[test]
    fn test_public_key_encode_decode() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let (pk, _) = keypair(&mut rng);
        
        let bytes = pk.to_bytes();
        assert_eq!(bytes.len(), 1184);
        
        let decoded = PublicKey::from_bytes(&bytes);
        assert_eq!(decoded.rho, pk.rho);
    }
    
    #[test]
    fn test_secret_key_encode_decode() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let (_, sk) = keypair(&mut rng);
        
        let bytes = sk.to_bytes();
        assert_eq!(bytes.len(), 1152);
        
        let decoded = SecretKey::from_bytes(&bytes);
        
        // Check first polynomial matches
        for i in 0..KYBER_N {
            assert_eq!(decoded.s[0].coeffs[i], sk.s[0].coeffs[i]);
        }
    }
    
    #[test]
    fn test_keypair_deterministic() {
        let mut rng1 = CryptoRng::new([42u8; 32]);
        let mut rng2 = CryptoRng::new([42u8; 32]);
        
        let (pk1, _) = keypair(&mut rng1);
        let (pk2, _) = keypair(&mut rng2);
        
        // Same seed should give same keys
        assert_eq!(pk1.rho, pk2.rho);
    }
    
    #[test]
    fn test_keypair_different_seeds() {
        let mut rng1 = CryptoRng::new([1u8; 32]);
        let mut rng2 = CryptoRng::new([2u8; 32]);
        
        let (pk1, _) = keypair(&mut rng1);
        let (pk2, _) = keypair(&mut rng2);
        
        // Different seeds should give different keys
        assert_ne!(pk1.rho, pk2.rho);
    }
}
