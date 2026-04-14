// SPDX-License-Identifier: MIT
//! QMI (Qualcomm MSM Interface) Protocol Engine
//!
//! Ported from Linux: `drivers/net/wwan/qmi_wwan.c`,
//!                    `lib/qmi_encode.c`, `include/linux/qmi_encdec.h`
//!
//! QMI framing: QMUX header + service-specific TLV payload.
//!
//! QMUX frame layout:
//! ┌───────────────────────────────────────────────┐
//! │ IF_TYPE (1) │ LEN (2 LE) │ FLAGS (1)          │
//! │ SVC_ID (1)  │ CLIENT_ID (1)                   │
//! │ ── SDU ─────────────────────────────────────── │
//! │ CTRL_FLAGS (1) │ TXN_ID (1 or 2) │ MSG_ID (2) │
//! │ TLV_LEN (2) │ TLV payload …                   │
//! └───────────────────────────────────────────────┘

use core::sync::atomic::{AtomicU16, Ordering};
use spin::Mutex;
use crate::error::KernelError;

// ---------------------------------------------------------------------------
// Service IDs
// ---------------------------------------------------------------------------
pub const QMI_CTL: u8 = 0x00; // Control service
pub const QMI_WDS: u8 = 0x01; // Wireless Data Service
pub const QMI_DMS: u8 = 0x02; // Device Management Service
pub const QMI_NAS: u8 = 0x03; // Network Access Service
pub const QMI_WMS: u8 = 0x05; // Wireless Messaging Service
pub const QMI_LOC: u8 = 0x10; // Location service

// ---------------------------------------------------------------------------
// Control message IDs
// ---------------------------------------------------------------------------
pub mod ctl_msg {
    pub const GET_VERSION_INFO:   u16 = 0x0021;
    pub const ALLOC_CLIENT_ID:    u16 = 0x0022;
    pub const RELEASE_CLIENT_ID:  u16 = 0x0023;
    pub const SYNC:               u16 = 0x0027;
    pub const SET_INSTANCE_ID:    u16 = 0x0028;
}

// ---------------------------------------------------------------------------
// WDS message IDs
// ---------------------------------------------------------------------------
pub mod wds_msg {
    pub const START_NETWORK_INTERFACE:  u16 = 0x0020;
    pub const STOP_NETWORK_INTERFACE:   u16 = 0x0021;
    pub const GET_PKT_SRVC_STATUS:      u16 = 0x0022;
    pub const GET_CURRENT_CHANNEL_RATE: u16 = 0x0023;
    pub const GET_RUNTIME_SETTINGS:     u16 = 0x002D;
    pub const SET_IP_FAMILY:            u16 = 0x004D;
}

// ---------------------------------------------------------------------------
// NAS message IDs
// ---------------------------------------------------------------------------
pub mod nas_msg {
    pub const GET_SIGNAL_STRENGTH:      u16 = 0x0020;
    pub const GET_SERVING_SYSTEM:       u16 = 0x0024;
    pub const GET_HOME_NETWORK:         u16 = 0x0025;
    pub const GET_RSSI:                 u16 = 0x003F;
    pub const REGISTER_INDICATIONS:     u16 = 0x003A;
    pub const GET_OPERATOR_NAME:        u16 = 0x0048;
}

// ---------------------------------------------------------------------------
// TLV type constants
// ---------------------------------------------------------------------------
pub mod tlv {
    pub const RESULT_CODE: u8 = 0x02;
    pub const APN_NAME:    u8 = 0x14;
    pub const USER:        u8 = 0x17;
    pub const PASS:        u8 = 0x18;
    pub const AUTH_PREF:   u8 = 0x16;
    pub const IP_FAMILY:   u8 = 0x19;
}

// ---------------------------------------------------------------------------
// Error codes
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum QmiError {
    None            = 0x0000,
    MalformedMsg    = 0x0001,
    NoMemory        = 0x0002,
    Internal        = 0x0003,
    Aborted         = 0x0004,
    ClientIdsExhausted = 0x0005,
    UnabortableTransaction = 0x0006,
    InvalidClientId = 0x0007,
    NoThresholdInfo = 0x0008,
    InvalidHandle   = 0x0009,
    InvalidProfile  = 0x000A,
    InvalidPinId    = 0x000B,
    IncorrectPin    = 0x000C,
    NoNetworkFound  = 0x000D,
    CallFailed      = 0x000E,
    OutOfCall       = 0x000F,
    NotProvisioned  = 0x0010,
    MissingArg      = 0x0011,
    ArgTooLong      = 0x0013,
    InvalidTxId     = 0x0014,
    DeviceInUse     = 0x0015,
    NetworkUnsupported = 0x0016,
    DeviceUnsupported = 0x0017,
    NoEffect        = 0x0018,
    NoFreeProfile   = 0x0019,
    InvalidPdpType  = 0x001A,
    InvalidTechPref = 0x001B,
    ProfileTypeNotSupported = 0x001C,
    ProfileIdAlreadyExists = 0x001D,
    ProfileDeleteError = 0x001E,
    Timeout         = 0x001F,
    Unknown         = 0xFFFF,
}

pub type QmiResult<T> = Result<T, QmiError>;

// ---------------------------------------------------------------------------
// QMUX frame structures
// ---------------------------------------------------------------------------

/// Maximum TLV payload inside a QMI message
pub const QMI_MAX_TLV_LEN: usize = 512;
/// Maximum number of TLVs per message
pub const QMI_MAX_TLVS: usize = 16;
/// Maximum length of a full QMUX frame we handle
pub const QMI_MAX_FRAME_LEN: usize = 600;

/// A single decoded TLV (Type–Length–Value)
#[derive(Clone, Copy)]
pub struct QmiTlv {
    pub tlv_type: u8,
    pub len:      u16,
    pub data:     [u8; 64], // max 64 bytes per TLV for embedded use
}

impl QmiTlv {
    pub const fn empty() -> Self {
        Self { tlv_type: 0, len: 0, data: [0u8; 64] }
    }

    pub fn payload(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }
}

/// Decoded QMI message (service-level SDU)
pub struct QmiMsg {
    pub svc_id:     u8,
    pub client_id:  u8,
    pub ctrl_flags: u8,
    pub txn_id:     u16,
    pub msg_id:     u16,
    pub tlv_count:  u8,
    pub tlvs:       [QmiTlv; QMI_MAX_TLVS],
}

impl QmiMsg {
    pub const fn new(svc_id: u8, client_id: u8, msg_id: u16) -> Self {
        Self {
            svc_id,
            client_id,
            ctrl_flags: 0,
            txn_id: 0,
            msg_id,
            tlv_count: 0,
            tlvs: [QmiTlv { tlv_type: 0, len: 0, data: [0u8; 64] }; QMI_MAX_TLVS],
        }
    }

    /// Add a TLV to the message.
    pub fn add_tlv(&mut self, tlv_type: u8, data: &[u8]) -> QmiResult<()> {
        if self.tlv_count as usize >= QMI_MAX_TLVS {
            return Err(QmiError::NoMemory);
        }
        if data.len() > 64 {
            return Err(QmiError::ArgTooLong);
        }
        let idx = self.tlv_count as usize;
        self.tlvs[idx].tlv_type = tlv_type;
        self.tlvs[idx].len = data.len() as u16;
        self.tlvs[idx].data[..data.len()].copy_from_slice(data);
        self.tlv_count += 1;
        Ok(())
    }

    /// Find a TLV by type, returning reference to its data slice.
    pub fn find_tlv(&self, tlv_type: u8) -> Option<&[u8]> {
        for i in 0..self.tlv_count as usize {
            if self.tlvs[i].tlv_type == tlv_type {
                return Some(self.tlvs[i].payload());
            }
        }
        None
    }

    /// Extract QMI result code from TLV 0x02.
    /// Format: result(2 LE) + error(2 LE)
    pub fn result_code(&self) -> QmiResult<()> {
        if let Some(d) = self.find_tlv(tlv::RESULT_CODE) {
            if d.len() < 4 { return Err(QmiError::MalformedMsg); }
            let result = u16::from_le_bytes([d[0], d[1]]);
            if result == 0 {
                return Ok(());
            }
            let err_code = u16::from_le_bytes([d[2], d[3]]);
            // Map known error codes
            return Err(match err_code {
                0x000D => QmiError::NoNetworkFound,
                0x0001 => QmiError::MalformedMsg,
                0x000E => QmiError::CallFailed,
                _      => QmiError::Unknown,
            });
        }
        Ok(()) // No result TLV → assume success
    }
}

// ---------------------------------------------------------------------------
// Client registry
// ---------------------------------------------------------------------------

/// A QMI client bound to a service on a device
#[derive(Clone, Copy)]
pub struct QmiClient {
    pub dev_id:    u8,
    pub svc_id:    u8,
    pub client_id: u8,
    pub allocated: bool,
}

impl QmiClient {
    pub const fn empty() -> Self {
        Self { dev_id: 0, svc_id: 0, client_id: 0, allocated: false }
    }
}

pub const MAX_QMI_CLIENTS: usize = 32;

pub struct QmiClientTable {
    pub clients: [QmiClient; MAX_QMI_CLIENTS],
    pub count: usize,
}

impl QmiClientTable {
    pub const fn new() -> Self {
        Self {
            clients: [QmiClient { dev_id: 0, svc_id: 0, client_id: 0, allocated: false }; MAX_QMI_CLIENTS],
            count: 0,
        }
    }
}

static QMI_CLIENTS: Mutex<QmiClientTable> = Mutex::new(QmiClientTable::new());
static NEXT_TXN: AtomicU16 = AtomicU16::new(1);

/// Allocate a client ID for a service.
pub fn qmi_alloc_client(dev_id: u8, svc_id: u8) -> QmiResult<u8> {
    let mut tbl = QMI_CLIENTS.lock();
    if tbl.count >= MAX_QMI_CLIENTS {
        return Err(QmiError::ClientIdsExhausted);
    }
    // Assign client_id = service-local sequential
    let mut max_id: u8 = 0;
    for i in 0..tbl.count {
        let c = &tbl.clients[i];
        if c.dev_id == dev_id && c.svc_id == svc_id && c.client_id > max_id {
            max_id = c.client_id;
        }
    }
    let new_id = max_id.wrapping_add(1);
    let idx = tbl.count;
    tbl.clients[idx] = QmiClient {
        dev_id,
        svc_id,
        client_id: new_id,
        allocated: true,
    };
    tbl.count += 1;
    Ok(new_id)
}

/// Release a client.
pub fn qmi_release_client(dev_id: u8, svc_id: u8, client_id: u8) -> QmiResult<()> {
    let mut tbl = QMI_CLIENTS.lock();
    for i in 0..tbl.count {
        let c = &tbl.clients[i];
        if c.dev_id == dev_id && c.svc_id == svc_id && c.client_id == client_id {
            // Swap-remove
            tbl.count -= 1;
            tbl.clients[i] = tbl.clients[tbl.count];
            return Ok(());
        }
    }
    Err(QmiError::InvalidClientId)
}

// ---------------------------------------------------------------------------
// Encode / Decode
// ---------------------------------------------------------------------------

/// Encode a QMI message into a QMUX frame buffer.
///
/// Returns the number of bytes written.
pub fn qmi_encode_tlv(msg: &mut QmiMsg, buf: &mut [u8; QMI_MAX_FRAME_LEN]) -> Result<usize, KernelError> {
    // Assign transaction ID
    msg.txn_id = NEXT_TXN.fetch_add(1, Ordering::Relaxed);

    // Build TLV payload first
    let mut tlv_buf = [0u8; QMI_MAX_TLV_LEN];
    let mut tlv_pos: usize = 0;
    for i in 0..msg.tlv_count as usize {
        let t = &msg.tlvs[i];
        if tlv_pos + 3 + t.len as usize > QMI_MAX_TLV_LEN {
            return Err(KernelError::InvalidParameter(""));
        }
        tlv_buf[tlv_pos] = t.tlv_type;
        tlv_pos += 1;
        tlv_buf[tlv_pos..tlv_pos + 2].copy_from_slice(&(t.len as u16).to_le_bytes());
        tlv_pos += 2;
        tlv_buf[tlv_pos..tlv_pos + t.len as usize].copy_from_slice(&t.data[..t.len as usize]);
        tlv_pos += t.len as usize;
    }

    // SDU size = ctrl_flags(1) + txn_id(1 for CTL / 2 for others) + msg_id(2) + tlv_len(2) + tlvs
    let txn_bytes: usize = if msg.svc_id == QMI_CTL { 1 } else { 2 };
    let sdu_len = 1 + txn_bytes + 2 + 2 + tlv_pos;

    // QMUX header = if_type(1) + len(2) + flags(1) + svc_id(1) + client_id(1) = 6 bytes
    let frame_len = 1 + 2 + 1 + 1 + 1 + sdu_len;
    if frame_len > QMI_MAX_FRAME_LEN {
        return Err(KernelError::InvalidParameter(""));
    }

    let mut pos = 0;
    buf[pos] = 0x01; pos += 1; // IF_TYPE = QMUX
    let qmux_len = (frame_len - 1) as u16; // length field excludes IF_TYPE byte
    buf[pos..pos+2].copy_from_slice(&qmux_len.to_le_bytes()); pos += 2;
    buf[pos] = 0x00; pos += 1; // FLAGS = host-to-modem
    buf[pos] = msg.svc_id; pos += 1;
    buf[pos] = msg.client_id; pos += 1;
    // SDU
    buf[pos] = 0x00; pos += 1; // CTRL_FLAGS = request
    if txn_bytes == 1 {
        buf[pos] = msg.txn_id as u8; pos += 1;
    } else {
        buf[pos..pos+2].copy_from_slice(&msg.txn_id.to_le_bytes()); pos += 2;
    }
    buf[pos..pos+2].copy_from_slice(&msg.msg_id.to_le_bytes()); pos += 2;
    buf[pos..pos+2].copy_from_slice(&(tlv_pos as u16).to_le_bytes()); pos += 2;
    buf[pos..pos+tlv_pos].copy_from_slice(&tlv_buf[..tlv_pos]); pos += tlv_pos;

    Ok(pos)
}

/// Decode a QMUX frame from raw bytes into a QmiMsg.
pub fn qmi_decode_tlv(raw: &[u8]) -> Result<QmiMsg, KernelError> {
    if raw.len() < 7 {
        return Err(KernelError::InvalidParameter(""));
    }
    if raw[0] != 0x01 {
        return Err(KernelError::InvalidParameter("")); // not QMUX
    }
    let svc_id    = raw[4];
    let client_id = raw[5];
    // SDU starts at offset 6
    let ctrl_flags = raw[6];
    let (txn_id, sdu_off) = if svc_id == QMI_CTL {
        (raw[7] as u16, 8usize)
    } else {
        if raw.len() < 9 { return Err(KernelError::InvalidParameter("")); }
        (u16::from_le_bytes([raw[7], raw[8]]), 9usize)
    };
    if raw.len() < sdu_off + 4 {
        return Err(KernelError::InvalidParameter(""));
    }
    let msg_id  = u16::from_le_bytes([raw[sdu_off], raw[sdu_off + 1]]);
    let tlv_len = u16::from_le_bytes([raw[sdu_off + 2], raw[sdu_off + 3]]) as usize;
    let mut tlv_pos = sdu_off + 4;

    let mut msg = QmiMsg::new(svc_id, client_id, msg_id);
    msg.ctrl_flags = ctrl_flags;
    msg.txn_id = txn_id;

    let end = (tlv_pos + tlv_len).min(raw.len());
    while tlv_pos + 3 <= end {
        let t_type = raw[tlv_pos];
        let t_len  = u16::from_le_bytes([raw[tlv_pos + 1], raw[tlv_pos + 2]]) as usize;
        tlv_pos += 3;
        if tlv_pos + t_len > end { break; }
        let _ = msg.add_tlv(t_type, &raw[tlv_pos..tlv_pos + t_len]);
        tlv_pos += t_len;
    }

    Ok(msg)
}

/// Send a QMI message via device MMIO and return a response (blocking stub).
///
/// In production this would use an IRQ-driven completion; here it encodes
/// the request and writes it to the device TX register.
pub fn qmi_send(dev_idx: u8, msg: &mut QmiMsg) -> Result<(), KernelError> {
    use crate::drivers::clk::mmio::write_reg;
    use super::qmi_wwan::qmi_wwan_base;

    let base = qmi_wwan_base(dev_idx);
    let mut buf = [0u8; QMI_MAX_FRAME_LEN];
    let len = qmi_encode_tlv(msg, &mut buf)?;

    // Write frame length then data to TX FIFO register
    write_reg(base + 0x00, len as u32); // TX_LEN
    for chunk in buf[..len].chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        write_reg(base + 0x04, u32::from_le_bytes(word));
    }
    Ok(())
}

/// Poll for a QMI response from the device RX register (blocking stub).
pub fn qmi_recv(dev_idx: u8) -> Result<QmiMsg, KernelError> {
    use crate::drivers::clk::mmio::read_reg;
    use super::qmi_wwan::qmi_wwan_base;

    let base = qmi_wwan_base(dev_idx);
    let len = read_reg(base + 0x08) as usize; // RX_LEN
    if len == 0 || len > QMI_MAX_FRAME_LEN {
        return Err(KernelError::Timeout);
    }
    let mut raw = [0u8; QMI_MAX_FRAME_LEN];
    let words = (len + 3) / 4;
    for i in 0..words {
        let w = read_reg(base + 0x0C + (i * 4) as u64);
        let off = i * 4;
        let end = (off + 4).min(len);
        raw[off..end].copy_from_slice(&w.to_le_bytes()[..end - off]);
    }
    qmi_decode_tlv(&raw[..len])
}

// ---------------------------------------------------------------------------
// Service helpers
// ---------------------------------------------------------------------------

/// A QMI service descriptor (service ID + name)
pub struct QmiService {
    pub svc_id:  u8,
    pub version: u32, // major(16) | minor(16)
}

impl QmiService {
    pub const fn new(svc_id: u8, major: u16, minor: u16) -> Self {
        Self {
            svc_id,
            version: ((major as u32) << 16) | (minor as u32),
        }
    }
    pub fn major(&self) -> u16 { (self.version >> 16) as u16 }
    pub fn minor(&self) -> u16 { self.version as u16 }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qmi_encode_decode_roundtrip() {
        let mut msg = QmiMsg::new(QMI_NAS, 0x01, nas_msg::GET_SIGNAL_STRENGTH);
        msg.add_tlv(0x10, &[0x01]).unwrap(); // custom TLV
        let mut buf = [0u8; QMI_MAX_FRAME_LEN];
        let len = qmi_encode_tlv(&mut msg, &mut buf).unwrap();
        assert!(len > 6, "frame too short");

        let decoded = qmi_decode_tlv(&buf[..len]).unwrap();
        assert_eq!(decoded.svc_id,    QMI_NAS);
        assert_eq!(decoded.client_id, 0x01);
        assert_eq!(decoded.msg_id,    nas_msg::GET_SIGNAL_STRENGTH);
        assert_eq!(decoded.tlv_count, 1);
        assert_eq!(decoded.find_tlv(0x10), Some([0x01u8].as_slice()));
    }

    #[test]
    fn test_qmi_add_too_many_tlvs() {
        let mut msg = QmiMsg::new(QMI_WDS, 0, wds_msg::START_NETWORK_INTERFACE);
        for i in 0..QMI_MAX_TLVS {
            assert!(msg.add_tlv(i as u8, &[0x00]).is_ok());
        }
        assert_eq!(msg.add_tlv(0xFF, &[0x00]), Err(QmiError::NoMemory));
    }

    #[test]
    fn test_qmi_result_code_ok() {
        let mut msg = QmiMsg::new(QMI_WDS, 1, wds_msg::GET_PKT_SRVC_STATUS);
        // result=0, error=0
        msg.add_tlv(tlv::RESULT_CODE, &[0x00, 0x00, 0x00, 0x00]).unwrap();
        assert_eq!(msg.result_code(), Ok(()));
    }

    #[test]
    fn test_qmi_result_code_no_network() {
        let mut msg = QmiMsg::new(QMI_NAS, 1, nas_msg::GET_SERVING_SYSTEM);
        // result=1, error=0x000D (no network)
        msg.add_tlv(tlv::RESULT_CODE, &[0x01, 0x00, 0x0D, 0x00]).unwrap();
        assert_eq!(msg.result_code(), Err(QmiError::NoNetworkFound));
    }

    #[test]
    fn test_qmi_alloc_release_client() {
        let cid = qmi_alloc_client(0, QMI_WDS).unwrap();
        assert!(cid > 0);
        assert!(qmi_release_client(0, QMI_WDS, cid).is_ok());
    }

    #[test]
    fn test_qmi_client_alloc_sequential() {
        let c1 = qmi_alloc_client(7, QMI_NAS).unwrap();
        let c2 = qmi_alloc_client(7, QMI_NAS).unwrap();
        assert_ne!(c1, c2);
        let _ = qmi_release_client(7, QMI_NAS, c1);
        let _ = qmi_release_client(7, QMI_NAS, c2);
    }

    #[test]
    fn test_qmi_decode_short_frame_fails() {
        let buf = [0x01u8, 0x05, 0x00];
        assert!(qmi_decode_tlv(&buf).is_err());
    }

    #[test]
    fn test_qmi_ctl_frame_txn_id_1_byte() {
        let mut msg = QmiMsg::new(QMI_CTL, 0x00, ctl_msg::SYNC);
        let mut buf = [0u8; QMI_MAX_FRAME_LEN];
        let len = qmi_encode_tlv(&mut msg, &mut buf).unwrap();
        let decoded = qmi_decode_tlv(&buf[..len]).unwrap();
        assert_eq!(decoded.svc_id, QMI_CTL);
        assert_eq!(decoded.msg_id, ctl_msg::SYNC);
    }
}
