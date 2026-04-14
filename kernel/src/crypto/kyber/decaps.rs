//! Kyber768 Decapsulation
//! 
//! Decapsulates shared secret using secret key

#![cfg_attr(not(feature = "std"), no_std)]

use super::poly::Poly;
use super::params::{KYBER_K, KYBER_N};
use super::keygen::{PublicKey, SecretKey};
use super::encaps::{Ciphertext, SharedSecret};
use super::shake::Shake256;

/// Decapsulate shared secret
/// 
/// Returns shared_secret
pub fn decapsulate(ct: &Ciphertext, sk: &SecretKey, pk: &PublicKey) -> SharedSecret {
    // Compute m' = v - s^T * u
    let mut mp = ct.v.clone();
    
    // Convert s to NTT domain
    let mut s_ntt = sk.s.clone();
    for poly in &mut s_ntt {
        poly.to_ntt();
    }
    
    // Convert u to NTT domain
    let mut u_ntt = ct.u.clone();
    for poly in &mut u_ntt {
        poly.to_ntt();
    }
    
    // Compute s^T * u
    for i in 0..KYBER_K {
        let mut temp = s_ntt[i].clone();
        temp.pointwise_mul(&u_ntt[i], 1);
        temp.from_ntt();
        mp.sub(&temp);
    }
    
    mp.reduce();
    
    // Decode message from polynomial
    let mut m = [0u8; 32];
    for i in 0..32 {
        for j in 0..8 {
            let coeff = mp.coeffs[8 * i + j];
            let bit = if coeff.abs() > (super::params::KYBER_Q / 4) {
                1
            } else {
                0
            };
            m[i] |= bit << j;
        }
    }
    
    // Derive shared secret from recovered message
    let mut ss = [0u8; 32];
    let mut shake = Shake256::new();
    shake.absorb(&m);
    shake.finalize();
    shake.squeeze(&mut ss);
    
    ss
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::rng::CryptoRng;
    use super::super::keygen::keypair;
    use super::super::encaps::encapsulate;
    
    #[test]
    fn test_decapsulation() {
        let mut rng = CryptoRng::new([1u8; 32]);
        let (pk, sk) = keypair(&mut rng);
        
        let (ct, ss_enc) = encapsulate(&pk, &mut rng);
        let ss_dec = decapsulate(&ct, &sk, &pk);
        
        // Shared secrets should match
        assert_eq!(ss_enc, ss_dec);
    }
    
    #[test]
    fn test_encaps_decaps_multiple() {
        let mut rng = CryptoRng::new([42u8; 32]);
        let (pk, sk) = keypair(&mut rng);
        
        // Test multiple encapsulations
        for _ in 0..5 {
            let (ct, ss_enc) = encapsulate(&pk, &mut rng);
            let ss_dec = decapsulate(&ct, &sk, &pk);
            assert_eq!(ss_enc, ss_dec);
        }
    }
    
    #[test]
    fn test_different_keypairs() {
        let mut rng = CryptoRng::new([1u8; 32]);
        
        let (pk1, sk1) = keypair(&mut rng);
        let (pk2, _) = keypair(&mut rng);
        
        let (ct, ss1) = encapsulate(&pk1, &mut rng);
        let ss2 = decapsulate(&ct, &sk1, &pk1);
        
        assert_eq!(ss1, ss2);
        
        // Different public key should give different ciphertext
        let (ct2, _) = encapsulate(&pk2, &mut rng);
        assert_ne!(ct.to_bytes(), ct2.to_bytes());
    }
}
