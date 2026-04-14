//! Kyber - Post-Quantum KEM
//! 
//! NIST PQC Round 3 Finalist
//! 
//! Complete Kyber768 implementation with key generation,
//! encapsulation, and decapsulation.
//! 
//! # Security Status
//! 
//! - **Algorithm:** Kyber768 (NIST Level 3 security)
//! - **Test Vectors:** ✅ NIST official test vectors passing
//! - **Side-Channel Protection:** ⚠️ Needs formal audit
//! - **Constant-Time Operations:** ⚠️ Needs verification
//! 
//! # Known Vulnerabilities
//! 
//! This implementation requires hardening against:
//! 
//! 1. **Timing Attacks:**
//!    - Rejection sampling in sampling.rs may leak information
//!    - Polynomial operations should use constant-time comparisons
//! 
//! 2. **Cache Attacks:**
//!    - Array indexing in NTT may create cache timing channels
//!    - Consider using constant-time table lookups
//! 
//! 3. **Power Analysis:**
//!    - Modular reduction operations may leak via power consumption
//!    - Requires hardware-level testing
//! 
//! # Recommendations for Production
//! 
//! - [ ] Implement constant-time rejection sampling
//! - [ ] Use ARM Crypto Extensions for NTT operations
//! - [ ] Add zeroization for all secret key material
//! - [ ] Conduct formal side-channel analysis
//! - [ ] Add runtime assertions for constant-time properties

pub mod params;
pub mod reduce;
pub mod ntt;
pub mod poly;
pub mod shake;
pub mod sampling;
pub mod keygen;
pub mod encaps;
pub mod decaps;

pub use params::*;
pub use reduce::*;
pub use ntt::*;
pub use poly::*;
pub use shake::*;
pub use sampling::*;
pub use keygen::*;
pub use encaps::*;
pub use decaps::*;
