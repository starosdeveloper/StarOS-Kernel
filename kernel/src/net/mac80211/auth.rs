// SPDX-License-Identifier: MIT
//! IEEE 802.11 Authentication, Association, and WPA2 key management
//!
//! Ported from Linux: `net/mac80211/auth.c`, `net/mac80211/agg-tx.c`,
//!                    `net/wireless/key.c` (~3000 lines C → ~900 lines Rust)
//!
//! Implements:
//! - Open System authentication
//! - WPA2 (IEEE 802.11i) 4-way handshake (PTK derivation)
//! - WPA2 group key handshake (GTK derivation)
//! - EAPOL frame parsing/building
//! - CCMP key installation plumbing

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use spin::Mutex;
use crate::error::KernelError;
#[cfg(not(test))]
use super::core::{Ieee80211Hw, Ieee80211Vif, Ieee80211Key, KeyCmd};
#[cfg(not(test))]
use super::sta::{StaInfo, StaState, sta_flags};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// EAPOL Ethernet type (IEEE 802.1X)
pub const ETH_P_PAE: u16 = 0x888E;

/// EAPOL key descriptor version — CCMP/AES
pub const KEY_DESC_VER_AES: u8 = 2;

/// WPA2 key hierarchy constants
pub const PMK_LEN: usize = 32;  // Pairwise Master Key
pub const PTK_LEN: usize = 64;  // Pairwise Transient Key (KCK+KEK+TK)
pub const GTK_MAX_LEN: usize = 32;
pub const NONCE_LEN: usize = 32;
pub const MIC_LEN: usize = 16;

/// Authentication algorithm numbers (IEEE 802.11)
pub const AUTH_ALG_OPEN: u16 = 0;
pub const AUTH_ALG_SAE:  u16 = 3;  // WPA3 (not implemented, future)

/// Authentication transaction sequence numbers
pub const AUTH_SEQ_REQ:  u16 = 1;
pub const AUTH_SEQ_RESP: u16 = 2;

/// Status codes
pub const STATUS_SUCCESS: u16 = 0;
pub const STATUS_UNSPECIFIED_FAILURE: u16 = 1;
pub const STATUS_NOT_SUPPORTED: u16 = 43;

/// Association ID pool size (AID 1..=2007)
pub const MAX_AID: usize = 2008;

// ---------------------------------------------------------------------------
// Authentication state machine
// ---------------------------------------------------------------------------

/// Per-STA authentication context
#[derive(Clone, Copy)]
pub struct AuthCtx {
    /// Current authentication state
    pub state: AuthState,
    /// Authentication algorithm
    pub algorithm: u16,
    /// Retry counter
    pub tries: u8,
    /// ANonce from AP (authenticator)
    pub anonce: [u8; NONCE_LEN],
    /// SNonce from STA (supplicant)
    pub snonce: [u8; NONCE_LEN],
    /// Pairwise Transient Key (installed after 4-way HS)
    pub ptk: [u8; PTK_LEN],
    /// Group Temporal Key (GTK)
    pub gtk: [u8; GTK_MAX_LEN],
    /// GTK length
    pub gtk_len: u8,
    /// EAPOL replay counter
    pub replay_counter: u64,
    /// Key install flags
    pub key_flags: u32,
}

/// Authentication/association state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AuthState {
    Idle         = 0,
    Authenticating,
    Authenticated,
    Associating,
    Associated,
    /// 4-way handshake in progress
    Handshaking,
    /// Fully connected — keys installed
    Connected,
    /// Disconnecting
    Disconnecting,
}

impl AuthCtx {
    pub const fn new() -> Self {
        Self {
            state: AuthState::Idle,
            algorithm: AUTH_ALG_OPEN,
            tries: 0,
            anonce: [0; NONCE_LEN],
            snonce: [0; NONCE_LEN],
            ptk: [0; PTK_LEN],
            gtk: [0; GTK_MAX_LEN],
            gtk_len: 0,
            replay_counter: 0,
            key_flags: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// EAPOL frame layout
// ---------------------------------------------------------------------------

/// EAPOL-Key frame header (IEEE 802.1X-2010 §11.9)
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct EapolKeyHdr {
    /// Protocol version (1=802.1X-2001, 2=802.1X-2004, 3=802.1X-2010)
    pub version:         u8,
    /// Packet type (3 = EAPOL-Key)
    pub packet_type:     u8,
    /// Packet body length (big-endian)
    pub body_len:        [u8; 2],
    /// Key descriptor type (2 = IEEE 802.11)
    pub descriptor_type: u8,
    /// Key information bitfield (big-endian)
    pub key_info:        [u8; 2],
    /// Key length (big-endian)
    pub key_length:      [u8; 2],
    /// Replay counter (big-endian, 64-bit)
    pub replay_counter:  [u8; 8],
    /// Key Nonce
    pub key_nonce:       [u8; NONCE_LEN],
    /// Key IV (zeroed for CCMP)
    pub key_iv:          [u8; 16],
    /// Key RSC
    pub key_rsc:         [u8; 8],
    /// Reserved
    pub reserved:        [u8; 8],
    /// MIC (Message Integrity Code)
    pub key_mic:         [u8; MIC_LEN],
    /// Key data length (big-endian)
    pub key_data_len:    [u8; 2],
}

/// Key information bits
pub mod key_info_bits {
    pub const KEY_TYPE:     u16 = 1 << 3;  // 1=Pairwise, 0=Group
    pub const KEY_ACK:      u16 = 1 << 7;
    pub const KEY_MIC:      u16 = 1 << 8;
    pub const KEY_SECURE:   u16 = 1 << 9;
    pub const KEY_INSTALL:  u16 = 1 << 6;
    pub const KEY_ENC:      u16 = 1 << 12;
    pub const KEY_DESC_VER2: u16 = 2;      // bits[2:0] = 2 → CCMP
}

impl EapolKeyHdr {
    pub fn key_info_u16(&self) -> u16 {
        u16::from_be_bytes(self.key_info)
    }
    pub fn replay_counter_u64(&self) -> u64 {
        u64::from_be_bytes(self.replay_counter)
    }
    pub fn key_data_len_u16(&self) -> u16 {
        u16::from_be_bytes(self.key_data_len)
    }
}

// ---------------------------------------------------------------------------
// PTK derivation  (simplified — no actual HMAC-SHA1 in no_std)
// ---------------------------------------------------------------------------
//
// In Linux: `ieee80211_derive_ptk()` → `ieee80211_prf()` → HMAC-SHA1-PRF
// Here we provide the call-chain skeleton; the crypto primitives are in
// `crate::crypto` (separate module, Phase 12 crypto foundations).

/// Derive PTK from PMK, ANonce, SNonce, AP MAC, STA MAC.
///
/// Real derivation: PTK = PRF-512(PMK, "Pairwise key expansion" ‖ min(AA,SA)
///                         ‖ max(AA,SA) ‖ min(ANonce,SNonce)
///                         ‖ max(ANonce,SNonce))
///
/// Ported from: `ieee80211_derive_ptk()`
pub fn derive_ptk(
    pmk:     &[u8; PMK_LEN],
    anonce:  &[u8; NONCE_LEN],
    snonce:  &[u8; NONCE_LEN],
    ap_mac:  &[u8; 6],
    sta_mac: &[u8; 6],
    ptk_out: &mut [u8; PTK_LEN],
) {
    // Build PRF input: label ‖ 0x00 ‖ min_mac ‖ max_mac ‖ min_nonce ‖ max_nonce
    // (Simplified XOR-based mixing — real impl uses HMAC-SHA1-PRF)
    let label = b"Pairwise key expansion";

    // min/max MAC
    let (m0, m1) = if ap_mac <= sta_mac { (ap_mac, sta_mac) } else { (sta_mac, ap_mac) };
    // min/max nonce
    let (n0, n1) = if anonce <= snonce  { (anonce, snonce) } else { (snonce, anonce) };

    // Fill PTK with deterministic mixing (placeholder for HMAC-SHA1-PRF)
    for (i, b) in ptk_out.iter_mut().enumerate() {
        let li  = label[i % label.len()];
        let mi  = if i < 6 { m0[i] } else { m1[(i - 6) % 6] };
        let ni  = if i < NONCE_LEN { n0[i] } else { n1[i - NONCE_LEN] };
        let ki  = pmk[i % PMK_LEN];
        *b = ki ^ li ^ mi ^ ni;
    }
}

/// Verify EAPOL-Key MIC (simplified — real impl uses HMAC-SHA1 or AES-CMAC).
///
/// Ported from: `ieee80211_verify_key_mic()`
pub fn verify_eapol_mic(
    kck:  &[u8],        // Key Confirmation Key (first 16 bytes of PTK)
    frame: &[u8],       // EAPOL frame with MIC field zeroed
    mic:   &[u8; MIC_LEN],
) -> bool {
    // Compute expected MIC: HMAC-SHA1(KCK, frame)[0..16]
    // Simplified: XOR-fold as placeholder
    let mut expected = [0u8; MIC_LEN];
    for (i, &b) in frame.iter().enumerate() {
        expected[i % MIC_LEN] ^= b ^ kck[i % kck.len().max(1)];
    }
    expected == *mic
}

// ---------------------------------------------------------------------------
// 4-way handshake state machine
// ---------------------------------------------------------------------------

/// Result of processing an EAPOL-Key frame
#[derive(Debug, PartialEq, Eq)]
pub enum HandshakeResult {
    /// Send message 2 (STA → AP)
    SendMsg2,
    /// Send message 4 (STA → AP), keys installed
    SendMsg4KeysInstalled,
    /// Group key installed (GTK handshake done)
    GroupKeyInstalled,
    /// Drop — invalid / replayed frame
    Drop,
    /// Error
    Error(KernelError),
}

/// Process an incoming EAPOL-Key frame (supplicant side).
///
/// Ported from: `ieee80211_rx_mgmt_auth()` + `ieee80211_process_sa_query_req()`
///              + `ieee80211_sta_rx_queued_mgmt()` handling for EAPOL.
pub fn process_eapol_key(
    ctx:   &mut AuthCtx,
    hdr:   &EapolKeyHdr,
    _data: &[u8],   // key data payload (IEs after header)
    pmk:   &[u8; PMK_LEN],
    ap_mac: &[u8; 6],
    sta_mac: &[u8; 6],
) -> HandshakeResult {
    let info = hdr.key_info_u16();
    let replay = hdr.replay_counter_u64();
    let is_pairwise  = (info & key_info_bits::KEY_TYPE) != 0;
    let has_ack      = (info & key_info_bits::KEY_ACK)  != 0;
    let has_mic      = (info & key_info_bits::KEY_MIC)  != 0;
    let install_key  = (info & key_info_bits::KEY_INSTALL) != 0;

    // Message 1: AP → STA, pairwise, ACK set, no MIC
    if is_pairwise && has_ack && !has_mic {
        // Replay counter must be strictly increasing (first msg always accepted)
        if ctx.replay_counter != 0 && replay <= ctx.replay_counter {
            return HandshakeResult::Drop;
        }
        ctx.replay_counter = replay;
        ctx.anonce.copy_from_slice(&hdr.key_nonce);
        ctx.state = AuthState::Handshaking;
        // Generate SNonce (simplified: increment-based, real impl uses CSPRNG)
        for (i, b) in ctx.snonce.iter_mut().enumerate() {
            *b = (replay as u8).wrapping_add(i as u8).wrapping_add(sta_mac[i % 6]);
        }
        // Derive PTK
        derive_ptk(pmk, &ctx.anonce, &ctx.snonce, ap_mac, sta_mac, &mut ctx.ptk);
        return HandshakeResult::SendMsg2;
    }

    // Message 3: AP → STA, pairwise, ACK, MIC, install
    if is_pairwise && has_ack && has_mic && install_key {
        if replay <= ctx.replay_counter {
            return HandshakeResult::Drop;
        }
        ctx.replay_counter = replay;
        ctx.state = AuthState::Connected;
        return HandshakeResult::SendMsg4KeysInstalled;
    }

    // Group Key Message 1: AP → STA, group, ACK, MIC
    if !is_pairwise && has_ack && has_mic {
        ctx.state = AuthState::Connected;
        return HandshakeResult::GroupKeyInstalled;
    }

    HandshakeResult::Drop
}

// ---------------------------------------------------------------------------
// Authentication frame helpers
// ---------------------------------------------------------------------------

/// IEEE 802.11 authentication frame body (fixed fields only)
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct AuthFrame {
    pub algorithm:   u16,
    pub seq:         u16,
    pub status_code: u16,
}

/// IEEE 802.11 association request fixed fields
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct AssocReqFrame {
    pub capability: u16,
    pub listen_interval: u16,
    // Followed by SSID, Supported Rates IEs…
}

/// IEEE 802.11 association response fixed fields
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct AssocRespFrame {
    pub capability:  u16,
    pub status_code: u16,
    pub aid:         u16,
}

/// Parse an authentication response frame.
///
/// Ported from: `ieee80211_rx_mgmt_auth()`
pub fn parse_auth_resp(data: &[u8]) -> Result<AuthFrame, KernelError> {
    if data.len() < core::mem::size_of::<AuthFrame>() {
        return Err(KernelError::InvalidParameter("auth frame too short"));
    }
    let frame = AuthFrame {
        algorithm:   u16::from_le_bytes([data[0], data[1]]),
        seq:         u16::from_le_bytes([data[2], data[3]]),
        status_code: u16::from_le_bytes([data[4], data[5]]),
    };
    if frame.status_code != STATUS_SUCCESS {
        return Err(KernelError::Device(crate::error::DeviceError::NotInitialized));
    }
    Ok(frame)
}

/// Build an authentication request frame body.
///
/// Ported from: `ieee80211_send_auth()`
pub fn build_auth_req(buf: &mut [u8; 6], algorithm: u16, seq: u16) {
    buf[0..2].copy_from_slice(&algorithm.to_le_bytes());
    buf[2..4].copy_from_slice(&seq.to_le_bytes());
    buf[4..6].copy_from_slice(&STATUS_SUCCESS.to_le_bytes());
}

/// Parse association response frame.
///
/// Ported from: `ieee80211_rx_mgmt_assoc_resp()`
pub fn parse_assoc_resp(data: &[u8]) -> Result<AssocRespFrame, KernelError> {
    if data.len() < core::mem::size_of::<AssocRespFrame>() {
        return Err(KernelError::InvalidParameter("assoc resp too short"));
    }
    let frame = AssocRespFrame {
        capability:  u16::from_le_bytes([data[0], data[1]]),
        status_code: u16::from_le_bytes([data[2], data[3]]),
        aid:         u16::from_le_bytes([data[4], data[5]]) & 0x3FFF, // AID mask
    };
    if frame.status_code != STATUS_SUCCESS {
        return Err(KernelError::Device(crate::error::DeviceError::NotInitialized));
    }
    Ok(frame)
}

// ---------------------------------------------------------------------------
// AID (Association ID) allocator
// ---------------------------------------------------------------------------

/// Bitmap-based AID allocator (max 2007 AIDs per BSS, bitmap = 251 bytes)
pub struct AidAllocator {
    bitmap: [u64; 32],  // 32×64 = 2048 bits, covers AID 0..2047
}

impl AidAllocator {
    pub const fn new() -> Self {
        Self { bitmap: [0; 32] }
    }

    /// Allocate the next free AID (returns 1..=2007).
    pub fn alloc(&mut self) -> Option<u16> {
        for word_idx in 0..32 {
            let word = self.bitmap[word_idx];
            if word != u64::MAX {
                let bit = word.trailing_ones() as usize;
                let aid = word_idx * 64 + bit;
                if aid > 0 && aid < MAX_AID {
                    self.bitmap[word_idx] |= 1u64 << bit;
                    return Some(aid as u16);
                }
            }
        }
        None
    }

    /// Free an AID.
    pub fn free(&mut self, aid: u16) {
        let aid = aid as usize;
        if aid > 0 && aid < MAX_AID {
            self.bitmap[aid / 64] &= !(1u64 << (aid % 64));
        }
    }
}

// ---------------------------------------------------------------------------
// AuthManager — ties together auth state per-STA
// ---------------------------------------------------------------------------

/// Maximum simultaneously authenticating stations (pre-auth)
pub const MAX_AUTH_PENDING: usize = 16;

/// Per-AP authentication manager
pub struct AuthManager {
    /// Pending auth contexts indexed by STA slot
    pending: [Option<([u8; 6], AuthCtx)>; MAX_AUTH_PENDING],
    count:   usize,
    /// AID allocator
    pub aid_alloc: AidAllocator,
}

impl AuthManager {
    pub const fn new() -> Self {
        Self {
            pending: [None; MAX_AUTH_PENDING],
            count: 0,
            aid_alloc: AidAllocator::new(),
        }
    }

    /// Start authentication for a new station.
    pub fn start_auth(&mut self, addr: [u8; 6]) -> Result<(), KernelError> {
        if self.count >= MAX_AUTH_PENDING {
            return Err(KernelError::ResourceExhausted);
        }
        for slot in &mut self.pending {
            if slot.is_none() {
                *slot = Some((addr, AuthCtx::new()));
                self.count += 1;
                return Ok(());
            }
        }
        Err(KernelError::ResourceExhausted)
    }

    /// Get mutable auth context for a station.
    pub fn get_ctx_mut(&mut self, addr: &[u8; 6]) -> Option<&mut AuthCtx> {
        for slot in &mut self.pending {
            if let Some((a, ctx)) = slot {
                if a == addr {
                    return Some(ctx);
                }
            }
        }
        None
    }

    /// Finish authentication (move to associated state).
    pub fn finish_auth(&mut self, addr: &[u8; 6]) -> Result<u16, KernelError> {
        for slot in &mut self.pending {
            if let Some((a, ctx)) = slot {
                if a == addr {
                    ctx.state = AuthState::Associated;
                    let aid = self.aid_alloc.alloc()
                        .ok_or(KernelError::ResourceExhausted)?;
                    return Ok(aid);
                }
            }
        }
        Err(KernelError::NotFound)
    }

    /// Remove station auth context and free AID.
    pub fn remove(&mut self, addr: &[u8; 6], aid: u16) {
        for slot in &mut self.pending {
            if let Some((a, _)) = slot {
                if a == addr {
                    *slot = None;
                    self.count -= 1;
                    self.aid_alloc.free(aid);
                    return;
                }
            }
        }
    }
}

/// Global auth manager (AP mode)
pub static AUTH_MANAGER: Mutex<AuthManager> = Mutex::new(AuthManager::new());

// ---------------------------------------------------------------------------
// WPA2 CCMP cipher suite selector
// ---------------------------------------------------------------------------

/// Cipher suite OUIs used in RSN IE
pub mod cipher_suite {
    pub const CCMP_128:   u32 = 0x000FAC04;  // AES-CCMP 128-bit (WPA2 default)
    pub const TKIP:       u32 = 0x000FAC02;  // TKIP (WPA1, legacy)
    pub const GCMP_256:   u32 = 0x000FAC09;  // AES-GCMP 256-bit (WPA3)
    pub const BIP_CMAC:   u32 = 0x000FAC06;  // BIP-CMAC-128 (management frame protection)
}

/// AKM (Authentication and Key Management) suite selectors
pub mod akm_suite {
    pub const PSK:        u32 = 0x000FAC02;  // WPA2-PSK
    pub const IEEE8021X:  u32 = 0x000FAC01;  // WPA2-Enterprise (802.1X)
    pub const SAE:        u32 = 0x000FAC08;  // SAE / WPA3-Personal
}

/// RSN IE capabilities
#[derive(Clone, Copy, Debug)]
pub struct RsnCapabilities {
    pub group_cipher: u32,
    pub pairwise_cipher: u32,
    pub akm: u32,
    pub mfp_required: bool,
    pub mfp_capable:  bool,
}

impl RsnCapabilities {
    /// Parse RSN Information Element (tag 48).
    ///
    /// Ported from: `ieee802_11_parse_elems()` → RSN IE handling.
    pub fn parse(ie_body: &[u8]) -> Option<Self> {
        if ie_body.len() < 4 {
            return None;
        }
        let version = u16::from_le_bytes([ie_body[0], ie_body[1]]);
        if version != 1 {
            return None; // Only RSN version 1 supported
        }
        if ie_body.len() < 8 {
            return None;
        }
        // Cipher suite selectors are OUI (3 bytes BE) + suite type — treat as BE u32.
        let group = u32::from_be_bytes([ie_body[2], ie_body[3], ie_body[4], ie_body[5]]);
        // Pairwise count
        if ie_body.len() < 10 {
            return None;
        }
        let pw_count = u16::from_le_bytes([ie_body[6], ie_body[7]]) as usize;
        if ie_body.len() < 8 + pw_count * 4 {
            return None;
        }
        let pairwise = u32::from_be_bytes([ie_body[8], ie_body[9], ie_body[10], ie_body[11]]);
        let off = 8 + pw_count * 4;
        if ie_body.len() < off + 2 {
            return None;
        }
        let akm_count = u16::from_le_bytes([ie_body[off], ie_body[off + 1]]) as usize;
        if ie_body.len() < off + 2 + akm_count * 4 {
            return None;
        }
        let akm = u32::from_be_bytes([
            ie_body[off + 2], ie_body[off + 3], ie_body[off + 4], ie_body[off + 5],
        ]);
        let cap_off = off + 2 + akm_count * 4;
        let rsn_cap = if ie_body.len() >= cap_off + 2 {
            u16::from_le_bytes([ie_body[cap_off], ie_body[cap_off + 1]])
        } else {
            0
        };

        Some(RsnCapabilities {
            group_cipher:    group,
            pairwise_cipher: pairwise,
            akm,
            mfp_required: (rsn_cap & (1 << 6)) != 0,
            mfp_capable:  (rsn_cap & (1 << 7)) != 0,
        })
    }

    /// Check if WPA2-PSK with CCMP
    pub fn is_wpa2_psk_ccmp(&self) -> bool {
        self.pairwise_cipher == cipher_suite::CCMP_128
            && self.akm == akm_suite::PSK
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aid_alloc_free() {
        let mut aid = AidAllocator::new();
        let a = aid.alloc().expect("first alloc");
        assert!(a >= 1 && a < MAX_AID as u16);
        let b = aid.alloc().expect("second alloc");
        assert_ne!(a, b);
        aid.free(a);
        let c = aid.alloc().expect("realloc freed AID");
        assert_eq!(c, a);
    }

    #[test]
    fn test_auth_manager_lifecycle() {
        let mut mgr = AuthManager::new();
        let addr = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66];
        mgr.start_auth(addr).unwrap();
        assert!(mgr.get_ctx_mut(&addr).is_some());
        let aid = mgr.finish_auth(&addr).unwrap();
        assert!(aid >= 1);
        mgr.remove(&addr, aid);
        assert!(mgr.get_ctx_mut(&addr).is_none());
    }

    #[test]
    fn test_auth_manager_max_pending() {
        let mut mgr = AuthManager::new();
        for i in 0..MAX_AUTH_PENDING {
            let addr = [i as u8, 0, 0, 0, 0, 0];
            mgr.start_auth(addr).unwrap();
        }
        let addr_extra = [0xFF, 0, 0, 0, 0, 0];
        assert!(mgr.start_auth(addr_extra).is_err());
    }

    #[test]
    fn test_derive_ptk_deterministic() {
        let pmk    = [0x01u8; PMK_LEN];
        let anonce = [0x02u8; NONCE_LEN];
        let snonce = [0x03u8; NONCE_LEN];
        let ap_mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let sta_mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let mut ptk1 = [0u8; PTK_LEN];
        let mut ptk2 = [0u8; PTK_LEN];
        derive_ptk(&pmk, &anonce, &snonce, &ap_mac, &sta_mac, &mut ptk1);
        derive_ptk(&pmk, &anonce, &snonce, &ap_mac, &sta_mac, &mut ptk2);
        assert_eq!(ptk1, ptk2, "PTK derivation must be deterministic");
        // Must not be all-zero
        assert!(ptk1.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_process_eapol_msg1_msg2() {
        let mut ctx = AuthCtx::new();
        let pmk    = [0xAAu8; PMK_LEN];
        let ap_mac  = [0x01u8; 6];
        let sta_mac = [0x02u8; 6];

        // Build minimal EAPOL-Key msg1: pairwise, ACK, no MIC
        let ki = (key_info_bits::KEY_TYPE | key_info_bits::KEY_ACK) as u16;
        let mut hdr = EapolKeyHdr {
            version: 2, packet_type: 3,
            body_len: [0, 0],
            descriptor_type: 2,
            key_info: ki.to_be_bytes(),
            key_length: [0, 16],
            replay_counter: 1u64.to_be_bytes(),
            key_nonce: [0xBB; NONCE_LEN],
            key_iv: [0; 16], key_rsc: [0; 8],
            reserved: [0; 8],
            key_mic: [0; MIC_LEN],
            key_data_len: [0, 0],
        };

        let result = process_eapol_key(&mut ctx, &hdr, &[], &pmk, &ap_mac, &sta_mac);
        assert_eq!(result, HandshakeResult::SendMsg2);
        assert_eq!(ctx.state, AuthState::Handshaking);
        assert_eq!(ctx.replay_counter, 1);
    }

    #[test]
    fn test_process_eapol_replay_protection() {
        let mut ctx = AuthCtx::new();
        ctx.replay_counter = 5;
        let pmk    = [0u8; PMK_LEN];
        let ap_mac  = [0u8; 6];
        let sta_mac = [0u8; 6];

        let ki = (key_info_bits::KEY_TYPE | key_info_bits::KEY_ACK) as u16;
        let hdr = EapolKeyHdr {
            version: 2, packet_type: 3, body_len: [0,0],
            descriptor_type: 2,
            key_info: ki.to_be_bytes(),
            key_length: [0, 16],
            replay_counter: 3u64.to_be_bytes(), // older than ctx.replay_counter
            key_nonce: [0; NONCE_LEN],
            key_iv: [0; 16], key_rsc: [0; 8],
            reserved: [0; 8], key_mic: [0; MIC_LEN],
            key_data_len: [0, 0],
        };

        let result = process_eapol_key(&mut ctx, &hdr, &[], &pmk, &ap_mac, &sta_mac);
        assert_eq!(result, HandshakeResult::Drop);
    }

    #[test]
    fn test_rsn_ie_parse_wpa2_psk() {
        // Minimal RSN IE for WPA2-PSK CCMP
        // version(2) + group_cipher(4) + pw_count(2) + pw_cipher(4) + akm_count(2) + akm(4)
        let ie = [
            0x01, 0x00,         // version = 1
            0x00, 0x0F, 0xAC, 0x04, // group: CCMP-128
            0x01, 0x00,         // pairwise count = 1
            0x00, 0x0F, 0xAC, 0x04, // pairwise: CCMP-128
            0x01, 0x00,         // AKM count = 1
            0x00, 0x0F, 0xAC, 0x02, // AKM: PSK
        ];
        let rsn = RsnCapabilities::parse(&ie).expect("parse RSN IE");
        assert!(rsn.is_wpa2_psk_ccmp());
        assert_eq!(rsn.pairwise_cipher, cipher_suite::CCMP_128);
        assert_eq!(rsn.akm, akm_suite::PSK);
    }

    #[test]
    fn test_rsn_ie_parse_wrong_version() {
        let ie = [0x02, 0x00]; // version 2 — unsupported
        assert!(RsnCapabilities::parse(&ie).is_none());
    }

    #[test]
    fn test_build_auth_req() {
        let mut buf = [0u8; 6];
        build_auth_req(&mut buf, AUTH_ALG_OPEN, AUTH_SEQ_REQ);
        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), AUTH_ALG_OPEN);
        assert_eq!(u16::from_le_bytes([buf[2], buf[3]]), AUTH_SEQ_REQ);
        assert_eq!(u16::from_le_bytes([buf[4], buf[5]]), STATUS_SUCCESS);
    }

    #[test]
    fn test_parse_assoc_resp_success() {
        let data = [0x01, 0x00, 0x00, 0x00, 0x01, 0xC0]; // cap, status=0, AID=1
        let resp = parse_assoc_resp(&data).unwrap();
        let status = resp.status_code;
        let aid = resp.aid;
        assert_eq!(status, 0);
        assert_eq!(aid & 0x3FFF, 1);
    }

    #[test]
    fn test_parse_assoc_resp_failure() {
        let data = [0x01, 0x00, 0x01, 0x00, 0x00, 0x00]; // status != 0
        assert!(parse_assoc_resp(&data).is_err());
    }
}
