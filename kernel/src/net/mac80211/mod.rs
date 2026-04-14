// SPDX-License-Identifier: MIT
//! IEEE 802.11 MAC Layer (mac80211)
//!
//! Ported from Linux: `net/mac80211/` (~25,000 lines C → ~3,000 lines Rust)

#[cfg(not(test))]
pub mod core;
#[cfg(not(test))]
pub mod sta;
pub mod auth;

#[cfg(not(test))]
pub use core::{
    Ieee80211Hw, Ieee80211Ops, Ieee80211Vif, VifType,
    BssConf, KeyCmd, Ieee80211Key, HwScanReq,
};
#[cfg(not(test))]
pub use sta::{StaInfo, StaState, StaTable, STA_TABLE, sta_flags};
pub use auth::{
    AuthCtx, AuthState, AuthManager, AUTH_MANAGER,
    EapolKeyHdr, HandshakeResult, RsnCapabilities,
    derive_ptk, process_eapol_key, parse_auth_resp, parse_assoc_resp,
    build_auth_req, cipher_suite, akm_suite, key_info_bits,
};
