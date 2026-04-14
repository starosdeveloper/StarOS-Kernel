//! Boot Image Signature Verification
//! 
//! Verifies post-quantum signatures on boot images

use crate::crypto::{SignPublicKey, SignKeyPair, CryptoError};
use crate::crypto::pq_sign::DetachedSignature;

/// Boot image signature header
#[repr(C)]
pub struct BootSignature {
    pub magic: [u8; 4],  // "PQSG"
    pub version: u32,
    pub sig_len: u32,
    pub pubkey_len: u32,
}

impl BootSignature {
    pub const MAGIC: [u8; 4] = *b"PQSG";
    
    pub fn verify_boot_image(image: &[u8], pubkey: &SignPublicKey) -> Result<(), CryptoError> {
        // Extract signature from end of image
        if image.len() < core::mem::size_of::<Self>() {
            return Err(CryptoError::InvalidSignature);
        }
        
        let sig_offset = image.len() - core::mem::size_of::<Self>();
        let header = unsafe { &*(image[sig_offset..].as_ptr() as *const Self) };
        
        if header.magic != Self::MAGIC {
            return Err(CryptoError::InvalidSignature);
        }
        
        // Signature is before header
        let sig_start = sig_offset - header.sig_len as usize;
        let sig_bytes = &image[sig_start..sig_offset];
        
        // Message is everything before signature
        let msg = &image[..sig_start];
        
        // Verify
        let sig = DetachedSignature::from_bytes(sig_bytes)?;
        
        pubkey.verify(msg, &sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_signature_verification() {
        let keypair = SignKeyPair::generate();
        let msg = b"test boot image";
        let sig = keypair.secret.sign(msg);
        
        assert!(keypair.public.verify(msg, &sig).is_ok());
    }
}
