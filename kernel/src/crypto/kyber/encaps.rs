//! Kyber768 Encapsulation
//! 
//! Encapsulates a shared secret using public key

#![cfg_attr(not(feature = "std"), no_std)]

use crate::crypto::rng::CryptoRng;
use super::poly::Poly;
use super::params::{KYBER_K, KYBER_ETA, KYBER_N};
use super::keygen::PublicKey;
use super::sampling::{sample_uniform, sample_cbd, prf};
use super::shake::Shake256;

/// Ciphertext for Kyber768
#[derive(Clone)]
pub struct Ciphertext {
    /// Polynomial vector u
    pub u: [Poly; KYBER_K],
    /// Polynomial v
    pub v: Poly,
}

/// Shared secret (32 bytes)
pub type SharedSecret = [u8; 32];

impl Ciphertext {
    /// Encode ciphertext to bytes (1088 bytes for Kyber768)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1088);
        
        // Encode u compressed (d=10, 320 bytes each)
        for poly in &self.u {
            bytes.extend_from_slice(&poly.compress(10));
        }
        
        // Encode v compressed (d=4, 128 bytes)
        bytes.extend_from_slice(&self.v.compress(4));
        
        bytes
    }
    
    /// Decode ciphertext from bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut u = [Poly::zero(); KYBER_K];
        
        // Decode u (3 * 320 = 960 bytes)
        for i in 0..KYBER_K {
            let start = i * 320;
            u[i] = Poly::decompress(&bytes[start..start + 320], 10);
        }
        
        // Decode v (128 bytes)
        let v = Poly::decompress(&bytes[960..1088], 4);
        
        Self { u, v }
    }
}

/// Encapsulate shared secret
/// 
/// Returns (ciphertext, shared_secret)
pub fn encapsulate(pk: &PublicKey, rng: &mut CryptoRng) -> (Ciphertext, SharedSecret) {
    // Generate random message
    let mut m = [0u8; 32];
    rng.fill_bytes(&mut m);
    
    // Derive randomness from message
    let mut coins = [0u8; 32];
    let mut shake = Shake256::new();
    shake.absorb(&m);
    shake.finalize();
    shake.squeeze(&mut coins);
    
    // Generate matrix A from public key rho
    let mut a = [[Poly::zero(); KYBER_K]; KYBER_K];
    for i in 0..KYBER_K {
        for j in 0..KYBER_K {
            a[i][j] = sample_uniform(&pk.rho, (i * KYBER_K + j) as u8);
        }
    }
    
    // Sample r vector
    let mut r = [Poly::zero(); KYBER_K];
    for i in 0..KYBER_K {
        let mut noise_buf = [0u8; KYBER_ETA * KYBER_N / 4];
        prf(&coins, i as u8, &mut noise_buf);
        r[i] = sample_cbd(&noise_buf, KYBER_ETA);
        r[i].to_ntt();
    }
    
    // Sample e1 vector
    let mut e1 = [Poly::zero(); KYBER_K];
    for i in 0..KYBER_K {
        let mut noise_buf = [0u8; KYBER_ETA * KYBER_N / 4];
        prf(&coins, (KYBER_K + i) as u8, &mut noise_buf);
        e1[i] = sample_cbd(&noise_buf, KYBER_ETA);
    }
    
    // Sample e2
    let mut noise_buf = [0u8; KYBER_ETA * KYBER_N / 4];
    prf(&coins, (2 * KYBER_K) as u8, &mut noise_buf);
    let mut e2 = sample_cbd(&noise_buf, KYBER_ETA);
    
    // Compute u = A^T * r + e1
    let mut u = [Poly::zero(); KYBER_K];
    for i in 0..KYBER_K {
        u[i] = Poly::zero();
        for j in 0..KYBER_K {
            let mut temp = a[j][i].clone(); // Transpose
            temp.pointwise_mul(&r[j], 1);
            u[i].add(&temp);
        }
        u[i].from_ntt();
        u[i].add(&e1[i]);
        u[i].reduce();
    }
    
    // Encode message to polynomial
    let mut msg_poly = Poly::zero();
    for i in 0..32 {
        for j in 0..8 {
            let bit = (m[i] >> j) & 1;
            msg_poly.coeffs[8 * i + j] = (bit as i16) * ((super::params::KYBER_Q + 1) / 2);
        }
    }
    
    // Compute v = t^T * r + e2 + msg
    let mut v = Poly::zero();
    for i in 0..KYBER_K {
        let mut temp = pk.t[i].clone();
        temp.pointwise_mul(&r[i], 1);
        v.add(&temp);
    }
    v.from_ntt();
    v.add(&e2);
    v.add(&msg_poly);
    v.reduce();
    
    // Derive shared secret
    let mut ss = [0u8; 32];
    let mut shake = Shake256::new();
    shake.absorb(&m);
    shake.finalize();
    shake.squeeze(&mut ss);
    
    let ct = Ciphertext { u, v };
    (ct, ss)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::rng::CryptoRng;
    use super::super::keygen::keypair;
    
    #[test]
    fn test_encapsulation() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let (pk, _) = keypair(&mut rng);
        
        let (ct, ss) = encapsulate(&pk, &mut rng);
        
        // Check dimensions
        assert_eq!(ct.u.len(), KYBER_K);
        assert_eq!(ss.len(), 32);
    }
    
    #[test]
    fn test_ciphertext_encode_decode() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let (pk, _) = keypair(&mut rng);
        let (ct, _) = encapsulate(&pk, &mut rng);
        
        let bytes = ct.to_bytes();
        assert_eq!(bytes.len(), 1088);
        
        let decoded = Ciphertext::from_bytes(&bytes);
        assert_eq!(decoded.u.len(), KYBER_K);
    }
    
    #[test]
    fn test_encapsulation_different_messages() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let (pk, _) = keypair(&mut rng);
        
        let (_, ss1) = encapsulate(&pk, &mut rng);
        let (_, ss2) = encapsulate(&pk, &mut rng);
        
        // Different random messages should give different secrets
        assert_ne!(ss1, ss2);
    }
    
    #[test]
    fn test_ciphertext_size() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let (pk, _) = keypair(&mut rng);
        let (ct, _) = encapsulate(&pk, &mut rng);
        
        let bytes = ct.to_bytes();
        
        // Kyber768: 3*320 + 128 = 1088 bytes
        assert_eq!(bytes.len(), 1088);
    }
}