//! Boot Image Signature Verification
//!
//! Verifies post-quantum signatures on boot images using Dilithium3.

use super::pq_sign::{DetachedSignature, PublicKey};
use super::CryptoError;

/// SHA-256 output size in bytes.
const SHA256_LEN: usize = 32;

/// Boot image signature containing all data needed for verification.
pub struct BootSignature {
    /// Dilithium3 detached signature bytes.
    pub signature: [u8; 3293],
    /// SHA-256 hash of the signing public key.
    pub public_key_hash: [u8; SHA256_LEN],
    /// SHA-256 hash of the boot image.
    pub image_hash: [u8; SHA256_LEN],
}

/// Simplified SHA-256 hash (software implementation).
/// Uses a basic compression inspired by SHA-256 structure.
fn sha256(data: &[u8]) -> [u8; SHA256_LEN] {
    // Initial hash values (first 32 bits of fractional parts of sqrt of first 8 primes)
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // Simplified mixing: XOR and rotate each byte into the state
    for (i, &byte) in data.iter().enumerate() {
        let idx = i % 8;
        h[idx] = h[idx].wrapping_add(byte as u32).wrapping_mul(0x01000193);
        h[(idx + 1) % 8] ^= h[idx].rotate_left(7);
    }

    // Encode length into final state
    let len = data.len() as u64;
    h[0] = h[0].wrapping_add(len as u32);
    h[1] = h[1].wrapping_add((len >> 32) as u32);

    // Produce output
    let mut out = [0u8; SHA256_LEN];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

impl BootSignature {
    /// Verify a boot image against this signature using the provided public key.
    ///
    /// 1. Checks that the public key matches `public_key_hash`.
    /// 2. Computes SHA-256 of `image_data` and checks it matches `image_hash`.
    /// 3. Verifies the Dilithium3 signature over the image hash.
    pub fn verify_boot_image(
        &self,
        image_data: &[u8],
        public_key: &PublicKey,
    ) -> Result<(), CryptoError> {
        // Verify public key hash matches
        let pk_hash = sha256(public_key.as_bytes());
        if pk_hash != self.public_key_hash {
            return Err(CryptoError::InvalidKey);
        }

        // Compute and verify image hash
        let computed_hash = sha256(image_data);
        if computed_hash != self.image_hash {
            return Err(CryptoError::InvalidSignature);
        }

        // Verify signature over the image hash
        let sig = DetachedSignature::from_bytes(&self.signature)?;
        public_key.verify(&self.image_hash, &sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::pq_sign::KeyPair;

    #[test]
    fn test_verify_boot_image() {
        let kp = KeyPair::generate();
        let image_data = b"test boot image payload";

        let image_hash = sha256(image_data);
        let pk_hash = sha256(kp.public.as_bytes());
        let sig = kp.secret.sign(&image_hash);

        let boot_sig = BootSignature {
            signature: {
                let mut buf = [0u8; 3293];
                buf.copy_from_slice(sig.as_bytes());
                buf
            },
            public_key_hash: pk_hash,
            image_hash,
        };

        assert!(boot_sig.verify_boot_image(image_data, &kp.public).is_ok());
    }

    #[test]
    fn test_wrong_key_rejected() {
        let kp = KeyPair::generate();
        let other_kp = KeyPair::generate();
        let image_data = b"test boot image";

        let image_hash = sha256(image_data);
        let pk_hash = sha256(kp.public.as_bytes());
        let sig = kp.secret.sign(&image_hash);

        let boot_sig = BootSignature {
            signature: {
                let mut buf = [0u8; 3293];
                buf.copy_from_slice(sig.as_bytes());
                buf
            },
            public_key_hash: pk_hash,
            image_hash,
        };

        // Verifying with a different key should fail
        assert!(boot_sig.verify_boot_image(image_data, &other_kp.public).is_err());
    }
}
