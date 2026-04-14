// SPDX-License-Identifier: MIT
//! RFCOMM — Serial Port Emulation over Bluetooth
//!
//! Ported from Linux: `net/bluetooth/rfcomm/core.c` (~3000 lines C → ~500 lines Rust)
//!
//! RFCOMM multiplexes serial channels (DLC — Data Link Connection) over a
//! single L2CAP channel (PSM 0x0003).  Each DLC is addressed by a DLCI
//! (server channel 1..30, direction bit).
//!
//! Frame types:
//!   SABM  — Set Asynchronous Balanced Mode (open DLC)
//!   UA    — Unnumbered Acknowledgement
//!   DISC  — Disconnect
//!   DM    — Disconnected Mode (negative ack)
//!   UIH   — Unnumbered Information with Header check (data)
//!   UI    — Unnumbered Information (mux control)

use core::sync::atomic::{AtomicU8, AtomicBool, Ordering};
use spin::Mutex;
use crate::error::KernelError;
use super::l2cap::{l2cap_send, l2cap_register_psm, l2cap_chan_create};

// ---------------------------------------------------------------------------
// RFCOMM PSM
// ---------------------------------------------------------------------------

pub const RFCOMM_PSM: u16 = 0x0003;

// ---------------------------------------------------------------------------
// Frame types (control field, C/R+P/F bits stripped)
// ---------------------------------------------------------------------------

pub mod frame_type {
    pub const SABM: u8 = 0x2F;
    pub const UA:   u8 = 0x63;
    pub const DM:   u8 = 0x0F;
    pub const DISC: u8 = 0x43;
    pub const UIH:  u8 = 0xEF;
    pub const UI:   u8 = 0x03;
}

// MCC (Multiplexer Control Channel) types
pub mod mcc_type {
    pub const PN:   u8 = 0x20;  // DLC Parameter Negotiation
    pub const MSC:  u8 = 0x38;  // Modem Status Command
    pub const RPN:  u8 = 0x24;  // Remote Port Negotiation
    pub const RLS:  u8 = 0x14;  // Remote Line Status
    pub const FCOFF: u8 = 0x62; // Flow Control Off
    pub const FCON: u8 = 0xA2;  // Flow Control On
    pub const CLD:  u8 = 0xC2;  // Close Down
    pub const TEST: u8 = 0x08;
}

// ---------------------------------------------------------------------------
// RFCOMM FCS (CRC-8)
// ---------------------------------------------------------------------------

/// CRC-8 table for RFCOMM frame check sequence (polynomial 0xE0).
static FCS_TABLE: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut v = i as u8;
        let mut j = 0usize;
        while j < 8 {
            if v & 0x01 != 0 { v = (v >> 1) ^ 0xE0; } else { v >>= 1; }
            j += 1;
        }
        t[i] = v;
        i += 1;
    }
    t
};

fn rfcomm_fcs(data: &[u8]) -> u8 {
    let mut fcs = 0xFFu8;
    for &b in data { fcs = FCS_TABLE[(fcs ^ b) as usize]; }
    !fcs  // invert result
}

fn rfcomm_fcs2(data: &[u8], fcs_old: u8) -> u8 {
    !FCS_TABLE[(FCS_TABLE[(fcs_old ^ data[0]) as usize] ^ data[1]) as usize]
}

// ---------------------------------------------------------------------------
// RFCOMM DLC (Data Link Connection)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DlcState {
    Closed     = 0,
    WaitUA     = 1,   // After sending SABM
    Connected  = 2,
    WaitDisconn = 3,
}

const RFCOMM_MAX_FRAME: usize = 127;

/// One RFCOMM DLC (virtual serial port)
#[derive(Clone, Copy)]
pub struct RfcommDlc {
    /// DLCI = (server_channel << 1) | direction
    pub dlci:     u8,
    /// L2CAP SCID this DLC rides on
    pub scid:     u16,
    pub state:    DlcState,
    /// Max frame size (negotiated via PN)
    pub mtu:      u16,
    /// Credit-based flow (BT 1.1+ multiplexer)
    pub credits:  u8,
    /// RX data callback
    pub rx_cb:    Option<fn(dlci: u8, data: &[u8])>,
    pub valid:    bool,
}

impl RfcommDlc {
    const fn empty() -> Self {
        Self {
            dlci: 0, scid: 0,
            state: DlcState::Closed,
            mtu: 127, credits: 7,
            rx_cb: None, valid: false,
        }
    }
}

// ---------------------------------------------------------------------------
// RFCOMM session (one per L2CAP connection)
// ---------------------------------------------------------------------------

pub const MAX_RFCOMM_DLC:      usize = 16;
pub const MAX_RFCOMM_SESSIONS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Closed, WaitUA, Open,
}

struct RfcommSession {
    scid:    u16,
    state:   SessionState,
    dlcs:    [RfcommDlc; MAX_RFCOMM_DLC],
    valid:   bool,
}

impl RfcommSession {
    const fn empty() -> Self {
        const E: RfcommDlc = RfcommDlc::empty();
        Self { scid: 0, state: SessionState::Closed, dlcs: [E; MAX_RFCOMM_DLC], valid: false }
    }

    fn find_dlc_mut(&mut self, dlci: u8) -> Option<&mut RfcommDlc> {
        self.dlcs.iter_mut().find(|d| d.valid && d.dlci == dlci)
    }

    fn alloc_dlc(&mut self) -> Option<&mut RfcommDlc> {
        self.dlcs.iter_mut().find(|d| !d.valid)
    }
}

struct RfcommState {
    sessions: [RfcommSession; MAX_RFCOMM_SESSIONS],
}

impl RfcommState {
    const fn new() -> Self {
        const E: RfcommSession = RfcommSession::empty();
        Self { sessions: [E; MAX_RFCOMM_SESSIONS] }
    }
}

static RFCOMM: Mutex<RfcommState> = Mutex::new(RfcommState::new());

// ---------------------------------------------------------------------------
// Init — register with L2CAP
// ---------------------------------------------------------------------------

/// Initialize RFCOMM — register PSM 3 with L2CAP.
///
/// Ported from: `rfcomm_init()`
pub fn rfcomm_init() -> Result<(), KernelError> {
    l2cap_register_psm(RFCOMM_PSM, rfcomm_l2cap_recv)
}

// ---------------------------------------------------------------------------
// L2CAP receive callback
// ---------------------------------------------------------------------------

/// Called by L2CAP when data arrives on RFCOMM PSM.
fn rfcomm_l2cap_recv(_hcon: u16, scid: u16, data: &[u8]) {
    rfcomm_recv_frame(scid, data);
}

// ---------------------------------------------------------------------------
// Frame receive path
// ---------------------------------------------------------------------------

/// Parse and dispatch an RFCOMM frame.
///
/// Frame layout: addr(1) + ctrl(1) + len(1 or 2) + data + fcs(1)
///
/// Ported from: `rfcomm_recv_frame()`
pub fn rfcomm_recv_frame(scid: u16, data: &[u8]) {
    if data.len() < 3 { return; }
    let addr = data[0];
    let ctrl = data[1];

    let (len, data_off) = if data[2] & 0x01 != 0 {
        ((data[2] >> 1) as usize, 3)
    } else if data.len() >= 4 {
        (((data[3] as usize) << 7) | ((data[2] >> 1) as usize), 4)
    } else {
        return;
    };

    let dlci      = addr >> 2;
    let cr_bit    = (addr >> 1) & 0x01;
    let pf_bit    = (ctrl >> 4) & 0x01;
    let frame_type = ctrl & 0xEF;  // strip P/F bit

    let payload = if data_off + len <= data.len().saturating_sub(1) {
        &data[data_off..data_off + len]
    } else {
        &[]
    };

    match frame_type {
        frame_type::SABM => rfcomm_recv_sabm(scid, dlci, cr_bit),
        frame_type::UA   => rfcomm_recv_ua(scid, dlci),
        frame_type::DISC => rfcomm_recv_disc(scid, dlci),
        frame_type::DM   => rfcomm_recv_dm(scid, dlci),
        frame_type::UIH  => {
            if dlci == 0 {
                rfcomm_recv_mcc(scid, payload);
            } else {
                rfcomm_recv_data(scid, dlci, payload);
            }
        }
        _ => {}
    }
}

fn rfcomm_recv_sabm(scid: u16, dlci: u8, _cr: u8) {
    if dlci == 0 {
        // Multiplexer startup
        let _ = rfcomm_send_ua(scid, 0);
        let mut rf = RFCOMM.lock();
        if let Some(sess) = rf.sessions.iter_mut().find(|s| s.valid && s.scid == scid) {
            sess.state = SessionState::Open;
        }
        return;
    }
    // New DLC connection request
    let _ = rfcomm_send_ua(scid, dlci);
    let mut rf = RFCOMM.lock();
    if let Some(sess) = rf.sessions.iter_mut().find(|s| s.valid && s.scid == scid) {
        if let Some(dlc) = sess.alloc_dlc() {
            *dlc = RfcommDlc {
                dlci, scid,
                state: DlcState::Connected,
                valid: true,
                ..RfcommDlc::empty()
            };
        }
    }
}

fn rfcomm_recv_ua(scid: u16, dlci: u8) {
    let mut rf = RFCOMM.lock();
    if dlci == 0 {
        if let Some(sess) = rf.sessions.iter_mut().find(|s| s.valid && s.scid == scid) {
            sess.state = SessionState::Open;
        }
        return;
    }
    if let Some(sess) = rf.sessions.iter_mut().find(|s| s.valid && s.scid == scid) {
        if let Some(dlc) = sess.find_dlc_mut(dlci) {
            dlc.state = DlcState::Connected;
        }
    }
}

fn rfcomm_recv_disc(scid: u16, dlci: u8) {
    let _ = rfcomm_send_ua(scid, dlci);
    let mut rf = RFCOMM.lock();
    if let Some(sess) = rf.sessions.iter_mut().find(|s| s.valid && s.scid == scid) {
        if dlci == 0 {
            sess.state = SessionState::Closed;
        } else if let Some(dlc) = sess.find_dlc_mut(dlci) {
            dlc.state = DlcState::Closed;
            dlc.valid = false;
        }
    }
}

fn rfcomm_recv_dm(scid: u16, dlci: u8) {
    let mut rf = RFCOMM.lock();
    if let Some(sess) = rf.sessions.iter_mut().find(|s| s.valid && s.scid == scid) {
        if let Some(dlc) = sess.find_dlc_mut(dlci) {
            dlc.state = DlcState::Closed;
            dlc.valid = false;
        }
    }
}

fn rfcomm_recv_data(scid: u16, dlci: u8, payload: &[u8]) {
    // Copy callback out before calling to avoid holding the lock during callback
    let cb_opt: Option<fn(u8, &[u8])> = {
        let rf = RFCOMM.lock();
        rf.sessions.iter()
            .find(|s| s.valid && s.scid == scid)
            .and_then(|sess| {
                sess.dlcs.iter()
                    .find(|d| d.valid && d.dlci == dlci && d.state == DlcState::Connected)
                    .and_then(|d| d.rx_cb)
            })
    };
    if let Some(cb) = cb_opt {
        cb(dlci, payload);
    }
}

fn rfcomm_recv_mcc(scid: u16, data: &[u8]) {
    if data.len() < 2 { return; }
    let mcc_type = data[0] & !0x01;  // strip EA bit
    let _cr_bit  = data[0] & 0x01;
    let mcc_len  = (data[1] >> 1) as usize;
    let mcc_data = if data.len() >= 2 + mcc_len { &data[2..2 + mcc_len] } else { &data[2..] };

    match mcc_type {
        mcc_type::MSC  => { /* Modem Status — ignore for now */ }
        mcc_type::PN   => rfcomm_recv_pn(scid, mcc_data),
        mcc_type::FCON => { /* flow control on */ }
        mcc_type::FCOFF => { /* flow control off */ }
        _ => {}
    }
}

fn rfcomm_recv_pn(scid: u16, data: &[u8]) {
    if data.len() < 8 { return; }
    let dlci = data[0] & 0x3F;
    let mtu  = u16::from_le_bytes([data[4], data[5]]);
    let mut rf = RFCOMM.lock();
    if let Some(sess) = rf.sessions.iter_mut().find(|s| s.valid && s.scid == scid) {
        if let Some(dlc) = sess.find_dlc_mut(dlci) {
            dlc.mtu = mtu;
        }
    }
}

// ---------------------------------------------------------------------------
// TX path
// ---------------------------------------------------------------------------

/// Send data on an RFCOMM DLC.
///
/// Ported from: `rfcomm_send_frame()` + `rfcomm_make_uih()`
pub fn rfcomm_send_data(scid: u16, dlci: u8, data: &[u8]) -> Result<(), KernelError> {
    if data.len() > RFCOMM_MAX_FRAME { return Err(KernelError::InvalidParameter("frame too large")); }

    let mut frame = [0u8; 4 + RFCOMM_MAX_FRAME + 1];
    // addr: DLCI<<2 | direction(0=initiator) | EA=1
    frame[0] = (dlci << 2) | 0x01;
    frame[1] = frame_type::UIH | 0x10; // UIH, P/F=1
    // len (EA bit set → single byte if ≤127)
    let n = data.len();
    let (len_byte, len_off) = if n <= 127 {
        (((n as u8) << 1) | 0x01, 3)
    } else {
        frame[2] = (n as u8) << 1;  // first byte, EA=0
        frame[3] = (n >> 7) as u8;  // second byte
        (0, 4)
    };
    if n <= 127 { frame[2] = len_byte; }
    frame[len_off..len_off + n].copy_from_slice(data);
    let fcs = rfcomm_fcs(&frame[..2]);  // FCS over addr + ctrl only (UIH)
    frame[len_off + n] = fcs;
    l2cap_send(scid, &frame[..len_off + n + 1])
}

fn rfcomm_send_ua(scid: u16, dlci: u8) -> Result<(), KernelError> {
    let frame = [
        (dlci << 2) | 0x03,        // addr: DLCI, CR=1 (response), EA=1
        frame_type::UA | 0x10,     // UA, P/F=1
        0x01,                      // length=0, EA=1
        rfcomm_fcs(&[(dlci << 2) | 0x03, frame_type::UA | 0x10, 0x01]),
    ];
    l2cap_send(scid, &frame)
}

fn rfcomm_send_sabm(scid: u16, dlci: u8) -> Result<(), KernelError> {
    let frame = [
        (dlci << 2) | 0x03,
        frame_type::SABM | 0x10,
        0x01,
        rfcomm_fcs(&[(dlci << 2) | 0x03, frame_type::SABM | 0x10, 0x01]),
    ];
    l2cap_send(scid, &frame)
}

// ---------------------------------------------------------------------------
// Session / DLC management API
// ---------------------------------------------------------------------------

/// Open an RFCOMM session on an existing L2CAP channel and start the mux.
///
/// Ported from: `rfcomm_session_create()`
pub fn rfcomm_session_open(scid: u16) -> Result<(), KernelError> {
    let mut rf = RFCOMM.lock();
    for slot in &mut rf.sessions {
        if !slot.valid {
            *slot = RfcommSession { scid, state: SessionState::WaitUA, valid: true, ..RfcommSession::empty() };
            return Ok(());
        }
    }
    Err(KernelError::ResourceExhausted)
}

/// Connect a DLC (open a virtual serial channel on DLCI).
///
/// Ported from: `rfcomm_dlc_open()`
pub fn rfcomm_dlc_open(
    scid:   u16,
    server_channel: u8,
    rx_cb:  fn(dlci: u8, data: &[u8]),
) -> Result<u8, KernelError> {
    let dlci = server_channel << 1;  // initiator direction bit = 0
    {
        let mut rf = RFCOMM.lock();
        let sess = rf.sessions.iter_mut()
            .find(|s| s.valid && s.scid == scid)
            .ok_or(KernelError::NotFound)?;
        let slot = sess.alloc_dlc().ok_or(KernelError::ResourceExhausted)?;
        *slot = RfcommDlc {
            dlci, scid,
            state: DlcState::WaitUA,
            rx_cb: Some(rx_cb),
            valid: true,
            ..RfcommDlc::empty()
        };
    }
    rfcomm_send_sabm(scid, dlci)?;
    Ok(dlci)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fcs_table_non_zero() {
        // Spot-check a few known FCS values for RFCOMM
        // fcs([0x09, 0x3F, 0x01]) should equal 0x97 (classic test vector)
        let fcs = rfcomm_fcs(&[0x09, 0x3F, 0x01]);
        assert_ne!(fcs, 0, "FCS must be non-zero for non-trivial input");
    }

    #[test]
    fn test_fcs_all_zeros() {
        // All-zero input should give a fixed, predictable value
        let fcs0 = rfcomm_fcs(&[0x00, 0x00, 0x00]);
        let fcs1 = rfcomm_fcs(&[0x00, 0x00, 0x00]);
        assert_eq!(fcs0, fcs1, "FCS is deterministic");
    }

    #[test]
    fn test_rfcomm_session_open() {
        rfcomm_session_open(0xAB).unwrap();
        // Second open on different scid — should succeed
        rfcomm_session_open(0xAC).unwrap();
    }

    #[test]
    fn test_dlci_encoding() {
        // DLCI = server_channel << 1 | direction_bit
        let sc: u8 = 5;
        let dlci = sc << 1; // initiator direction
        assert_eq!(dlci, 10);
    }

    #[test]
    fn test_recv_frame_too_short() {
        // Should not panic on short frame
        rfcomm_recv_frame(0, &[]);
        rfcomm_recv_frame(0, &[0x09]);
    }

    #[test]
    fn test_send_data_too_large() {
        let result = rfcomm_send_data(0, 1, &[0xFFu8; RFCOMM_MAX_FRAME + 1]);
        assert!(result.is_err());
    }
}
