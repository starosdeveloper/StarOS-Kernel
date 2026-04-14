//! Post-Quantum Cryptography Module
//! 
//! Provides Kyber (KEM) and Dilithium (signatures) for kernel security

pub mod pq_kem;
pub mod pq_sign;
pub mod boot_verify;

pub use pq_kem::{KeyPair as KemKeyPair, PublicKey as KemPublicKey, SecretKey as KemSecretKey};
pub use pq_sign::{KeyPair as SignKeyPair, PublicKey as SignPublicKey, SecretKey as SignSecretKey};
pub use boot_verify::BootSignature;

/// Crypto operation result
pub type Result<T> = core::result::Result<T, CryptoError>;

#[derive(Debug, Clone, Copy)]
pub enum CryptoError {
    InvalidKey,
    InvalidSignature,
    EncapsulationFailed,
    DecapsulationFailed,
}
