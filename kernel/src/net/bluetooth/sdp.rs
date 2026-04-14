// SPDX-License-Identifier: MIT
//! SDP — Service Discovery Protocol
//!
//! Ported from Linux: `net/bluetooth/sdp.c` / BlueZ user-space SDP (~3000 lines)
//!
//! Provides:
//! - Service record registration (static table, no heap)
//! - SDP PDU building/parsing (ServiceSearch, AttributeSearch,
//!   ServiceSearchAttribute)
//! - PSM 1 L2CAP handler integration

use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use crate::error::KernelError;
use super::l2cap::{l2cap_register_psm, l2cap_send};

// ---------------------------------------------------------------------------
// SDP PSM
// ---------------------------------------------------------------------------

pub const SDP_PSM: u16 = 0x0001;

// ---------------------------------------------------------------------------
// SDP PDU types
// ---------------------------------------------------------------------------

pub mod pdu_id {
    pub const ERROR_RSP:              u8 = 0x01;
    pub const SERVICE_SEARCH_REQ:     u8 = 0x02;
    pub const SERVICE_SEARCH_RSP:     u8 = 0x03;
    pub const SERVICE_ATTR_REQ:       u8 = 0x04;
    pub const SERVICE_ATTR_RSP:       u8 = 0x05;
    pub const SERVICE_SEARCH_ATTR_REQ: u8 = 0x06;
    pub const SERVICE_SEARCH_ATTR_RSP: u8 = 0x07;
}

// ---------------------------------------------------------------------------
// Well-known UUIDs (16-bit aliases of 128-bit Bluetooth Base UUID)
// ---------------------------------------------------------------------------

pub mod uuid16 {
    pub const SDP:            u16 = 0x0001;
    pub const RFCOMM:         u16 = 0x0003;
    pub const OBEX:           u16 = 0x0008;
    pub const L2CAP:          u16 = 0x0100;
    pub const HID:            u16 = 0x0011;
    pub const A2DP_SINK:      u16 = 0x110B;
    pub const A2DP_SOURCE:    u16 = 0x110A;
    pub const AVRCP_TARGET:   u16 = 0x110C;
    pub const AVRCP_CONTROL:  u16 = 0x110E;
    pub const HEADSET:        u16 = 0x1108;
    pub const HSP_HS:         u16 = 0x1108;
    pub const HSP_AG:         u16 = 0x1112;
    pub const HFP_HF:         u16 = 0x111E;
    pub const HFP_AG:         u16 = 0x111F;
    pub const SPP:            u16 = 0x1101;  // Serial Port Profile
    pub const PAN_NAP:        u16 = 0x1116;
    pub const PAN_PANU:       u16 = 0x1115;
}

// Standard attribute IDs
pub mod attr_id {
    pub const SERVICE_RECORD_HANDLE:       u16 = 0x0000;
    pub const SERVICE_CLASS_ID_LIST:       u16 = 0x0001;
    pub const SERVICE_RECORD_STATE:        u16 = 0x0002;
    pub const SERVICE_ID:                  u16 = 0x0003;
    pub const PROTOCOL_DESCRIPTOR_LIST:    u16 = 0x0004;
    pub const BROWSE_GROUP_LIST:           u16 = 0x0005;
    pub const LANGUAGE_BASE_ATTR_ID_LIST:  u16 = 0x0006;
    pub const SERVICE_INFO_TIME_TO_LIVE:   u16 = 0x0007;
    pub const SERVICE_AVAILABILITY:        u16 = 0x0008;
    pub const BT_PROFILE_DESCRIPTOR_LIST:  u16 = 0x0009;
    pub const SERVICE_NAME:                u16 = 0x0100;  // + base lang offset
    pub const SERVICE_DESCRIPTION:         u16 = 0x0101;
    pub const PROVIDER_NAME:               u16 = 0x0102;
}

// ---------------------------------------------------------------------------
// SDP data element encoding
// ---------------------------------------------------------------------------

/// SDP data element type descriptors (type | size_index in bits[7:3|2:0])
pub mod de_type {
    pub const NIL:     u8 = 0x00;
    pub const UINT:    u8 = 0x08;  // uint, size = 1<<(n&7)
    pub const INT:     u8 = 0x10;
    pub const UUID:    u8 = 0x18;
    pub const TEXT:    u8 = 0x20;
    pub const BOOL:    u8 = 0x28;
    pub const SEQ:     u8 = 0x30;  // sequence, followed by length
    pub const ALT:     u8 = 0x38;
    pub const URL:     u8 = 0x40;

    // size index
    pub const SIZE_1:  u8 = 0x00;  // 1 byte (only for fixed types)
    pub const SIZE_2:  u8 = 0x01;
    pub const SIZE_4:  u8 = 0x02;
    pub const SIZE_8:  u8 = 0x03;
    pub const SIZE_U8: u8 = 0x05;  // next byte = length
    pub const SIZE_U16:u8 = 0x06;  // next 2 bytes = length
}

// ---------------------------------------------------------------------------
// Compact service record (no heap — fixed max attributes)
// ---------------------------------------------------------------------------

pub const MAX_ATTRS:    usize = 16;
pub const MAX_ATTR_DATA: usize = 64;
pub const MAX_SERVICE_RECORDS: usize = 16;

/// One attribute in a service record
#[derive(Clone, Copy)]
pub struct SdpAttr {
    pub id:      u16,
    /// Pre-encoded SDP data element bytes
    pub data:    [u8; MAX_ATTR_DATA],
    pub data_len: u8,
    pub valid:   bool,
}

impl SdpAttr {
    const fn empty() -> Self {
        Self { id: 0, data: [0; MAX_ATTR_DATA], data_len: 0, valid: false }
    }
}

/// One service record
#[derive(Clone, Copy)]
pub struct ServiceRecord {
    pub handle: u32,
    pub attrs:  [SdpAttr; MAX_ATTRS],
    pub valid:  bool,
}

impl ServiceRecord {
    const fn empty() -> Self {
        const EA: SdpAttr = SdpAttr::empty();
        Self { handle: 0, attrs: [EA; MAX_ATTRS], valid: false }
    }

    /// Add an attribute (replaces existing if same id).
    pub fn add_attr(&mut self, id: u16, data: &[u8]) -> Result<(), KernelError> {
        let n = data.len().min(MAX_ATTR_DATA);
        // Update existing
        for attr in &mut self.attrs {
            if attr.valid && attr.id == id {
                attr.data[..n].copy_from_slice(&data[..n]);
                attr.data_len = n as u8;
                return Ok(());
            }
        }
        // Insert new
        for attr in &mut self.attrs {
            if !attr.valid {
                attr.id = id;
                attr.data[..n].copy_from_slice(&data[..n]);
                attr.data_len = n as u8;
                attr.valid = true;
                return Ok(());
            }
        }
        Err(KernelError::ResourceExhausted)
    }

    /// Find an attribute.
    pub fn get_attr(&self, id: u16) -> Option<&[u8]> {
        self.attrs.iter()
            .find(|a| a.valid && a.id == id)
            .map(|a| &a.data[..a.data_len as usize])
    }
}

// ---------------------------------------------------------------------------
// Service record table
// ---------------------------------------------------------------------------

static NEXT_HANDLE: AtomicU32 = AtomicU32::new(0x0001_0001);

struct SdpDb {
    records: [ServiceRecord; MAX_SERVICE_RECORDS],
}

impl SdpDb {
    const fn new() -> Self {
        const E: ServiceRecord = ServiceRecord::empty();
        Self { records: [E; MAX_SERVICE_RECORDS] }
    }
}

static SDP_DB: Mutex<SdpDb> = Mutex::new(SdpDb::new());

/// Register a service record. Returns its handle.
///
/// Ported from: `sdp_service_register()`
pub fn sdp_register_service(record: ServiceRecord) -> Result<u32, KernelError> {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let mut db = SDP_DB.lock();
    for slot in &mut db.records {
        if !slot.valid {
            *slot = record;
            slot.handle = handle;
            slot.valid = true;
            return Ok(handle);
        }
    }
    Err(KernelError::ResourceExhausted)
}

/// Unregister a service record by handle.
pub fn sdp_unregister_service(handle: u32) {
    let mut db = SDP_DB.lock();
    for slot in &mut db.records {
        if slot.valid && slot.handle == handle {
            slot.valid = false;
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience record builders
// ---------------------------------------------------------------------------

/// Build a minimal SPP (Serial Port Profile) service record.
///
/// Ported from: BlueZ `sdp_record_alloc()` + SPP profile record.
pub fn sdp_build_spp_record(rfcomm_channel: u8) -> ServiceRecord {
    let mut rec = ServiceRecord::empty();

    // Service Class ID List: [UUID16(SPP)]
    let svc_class = [
        de_type::SEQ | de_type::SIZE_U8, 3,          // sequence, 3 bytes
        de_type::UUID | de_type::SIZE_2,              // UUID16
        (uuid16::SPP >> 8) as u8, uuid16::SPP as u8,
    ];
    let _ = rec.add_attr(attr_id::SERVICE_CLASS_ID_LIST, &svc_class);

    // Protocol Descriptor List: [[L2CAP], [RFCOMM, u8(channel)]]
    let proto = [
        de_type::SEQ | de_type::SIZE_U8, 13,
        // L2CAP
        de_type::SEQ | de_type::SIZE_U8, 3,
        de_type::UUID | de_type::SIZE_2, 0x01, 0x00,
        // RFCOMM + channel
        de_type::SEQ | de_type::SIZE_U8, 5,
        de_type::UUID | de_type::SIZE_2, 0x00, 0x03,
        de_type::UINT | de_type::SIZE_1, rfcomm_channel,
    ];
    let _ = rec.add_attr(attr_id::PROTOCOL_DESCRIPTOR_LIST, &proto);

    rec
}

// ---------------------------------------------------------------------------
// SDP PDU dispatcher (L2CAP PSM 1 receive handler)
// ---------------------------------------------------------------------------

/// Initialize SDP — register with L2CAP on PSM 1.
///
/// Ported from: `sdp_init()`
pub fn sdp_init() -> Result<(), KernelError> {
    l2cap_register_psm(SDP_PSM, sdp_l2cap_recv)
}

fn sdp_l2cap_recv(_hcon: u16, scid: u16, data: &[u8]) {
    sdp_recv_pdu(scid, data);
}

/// Parse and respond to an SDP PDU.
///
/// Ported from: `sdp_process()`
pub fn sdp_recv_pdu(scid: u16, data: &[u8]) {
    if data.len() < 5 { return; }
    let pdu_id  = data[0];
    let tid     = u16::from_be_bytes([data[1], data[2]]);
    let _plen   = u16::from_be_bytes([data[3], data[4]]);
    let params  = &data[5..];

    match pdu_id {
        pdu_id::SERVICE_SEARCH_REQ      => handle_service_search(scid, tid, params),
        pdu_id::SERVICE_ATTR_REQ        => handle_service_attr(scid, tid, params),
        pdu_id::SERVICE_SEARCH_ATTR_REQ => handle_service_search_attr(scid, tid, params),
        _ => send_error_rsp(scid, tid, 0x0003), // Invalid PDU size / unknown PDU
    }
}

// ---------------------------------------------------------------------------
// PDU handlers
// ---------------------------------------------------------------------------

fn handle_service_search(scid: u16, tid: u16, _params: &[u8]) {
    // Simplified: return all registered record handles
    let db = SDP_DB.lock();
    let mut buf = [0u8; 7 + MAX_SERVICE_RECORDS * 4];
    // Response header: pdu(1) + tid(2) + plen(2) + total(2) + current(2)
    buf[0] = pdu_id::SERVICE_SEARCH_RSP;
    buf[1..3].copy_from_slice(&tid.to_be_bytes());
    let mut n = 0u16;
    let mut off = 7usize;
    for rec in db.records.iter().filter(|r| r.valid) {
        buf[off..off + 4].copy_from_slice(&rec.handle.to_be_bytes());
        off += 4;
        n += 1;
    }
    let plen = (2 + 2 + n * 4 + 1) as u16; // total_cnt + current_cnt + handles + cont_state
    buf[3..5].copy_from_slice(&plen.to_be_bytes());
    buf[5..7].copy_from_slice(&n.to_be_bytes()); // total service count
    // current service count (same, no continuation)
    let cur_off = 7 + n as usize * 4;
    if cur_off < buf.len() {
        buf[cur_off] = 0; // continuation state = 0
    }
    let _ = l2cap_send(scid, &buf[..cur_off + 1]);
}

fn handle_service_attr(scid: u16, tid: u16, params: &[u8]) {
    if params.len() < 4 { return; }
    let handle = u32::from_be_bytes([params[0], params[1], params[2], params[3]]);

    let db = SDP_DB.lock();
    let rec = match db.records.iter().find(|r| r.valid && r.handle == handle) {
        Some(r) => r,
        None    => { drop(db); send_error_rsp(scid, tid, 0x0002); return; }
    };

    // Serialize all attributes
    let mut body = [0u8; 512];
    let body_len = serialize_attrs(rec, &mut body);
    drop(db);

    send_attr_rsp(scid, tid, pdu_id::SERVICE_ATTR_RSP, &body[..body_len]);
}

fn handle_service_search_attr(scid: u16, tid: u16, _params: &[u8]) {
    // Simplified: serialize all service records' attributes
    let db = SDP_DB.lock();
    let mut body = [0u8; 1024];
    let mut total = 0usize;
    for rec in db.records.iter().filter(|r| r.valid) {
        total += serialize_attrs(rec, &mut body[total..]);
    }
    drop(db);
    send_attr_rsp(scid, tid, pdu_id::SERVICE_SEARCH_ATTR_RSP, &body[..total]);
}

fn serialize_attrs(rec: &ServiceRecord, out: &mut [u8]) -> usize {
    let mut off = 0usize;
    for attr in rec.attrs.iter().filter(|a| a.valid) {
        if off + 3 + attr.data_len as usize > out.len() { break; }
        // Attribute ID: uint16
        out[off]     = de_type::UINT | de_type::SIZE_2;
        out[off + 1] = (attr.id >> 8) as u8;
        out[off + 2] = attr.id as u8;
        off += 3;
        let n = attr.data_len as usize;
        out[off..off + n].copy_from_slice(&attr.data[..n]);
        off += n;
    }
    off
}

fn send_attr_rsp(scid: u16, tid: u16, pdu_id_byte: u8, attrs: &[u8]) {
    // pdu(1) + tid(2) + plen(2) + attr_list_byte_count(2) + attrs + cont_state(1)
    let plen = (2 + attrs.len() + 1) as u16;
    let mut hdr = [0u8; 7];
    hdr[0] = pdu_id_byte;
    hdr[1..3].copy_from_slice(&tid.to_be_bytes());
    hdr[3..5].copy_from_slice(&plen.to_be_bytes());
    hdr[5..7].copy_from_slice(&(attrs.len() as u16).to_be_bytes());
    // Build full response: hdr + attrs + cont_state(0)
    let mut buf = [0u8; 7 + 1024 + 1];
    buf[..7].copy_from_slice(&hdr);
    let n = attrs.len().min(1024);
    buf[7..7 + n].copy_from_slice(&attrs[..n]);
    buf[7 + n] = 0; // no continuation
    let _ = l2cap_send(scid, &buf[..7 + n + 1]);
}

fn send_error_rsp(scid: u16, tid: u16, error_code: u16) {
    let mut buf = [0u8; 7];
    buf[0] = pdu_id::ERROR_RSP;
    buf[1..3].copy_from_slice(&tid.to_be_bytes());
    buf[3..5].copy_from_slice(&2_u16.to_be_bytes()); // plen = 2
    buf[5..7].copy_from_slice(&error_code.to_be_bytes());
    let _ = l2cap_send(scid, &buf);
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdp_register_unregister() {
        let rec = ServiceRecord::empty();
        let handle = sdp_register_service(rec).unwrap();
        assert!(handle >= 0x0001_0001);
        sdp_unregister_service(handle);
        // Re-register — handle pool still has room
        let handle2 = sdp_register_service(ServiceRecord::empty()).unwrap();
        assert!(handle2 > handle);
        sdp_unregister_service(handle2);
    }

    #[test]
    fn test_build_spp_record() {
        let rec = sdp_build_spp_record(3);
        // Must have service class and protocol attrs
        assert!(rec.get_attr(attr_id::SERVICE_CLASS_ID_LIST).is_some());
        assert!(rec.get_attr(attr_id::PROTOCOL_DESCRIPTOR_LIST).is_some());
        // Protocol descriptor must contain RFCOMM channel 3
        let proto = rec.get_attr(attr_id::PROTOCOL_DESCRIPTOR_LIST).unwrap();
        assert!(proto.contains(&3u8), "channel 3 should be in proto descriptor");
    }

    #[test]
    fn test_service_record_add_attr() {
        let mut rec = ServiceRecord::empty();
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        rec.add_attr(0x1234, &data).unwrap();
        let got = rec.get_attr(0x1234).unwrap();
        assert_eq!(got, &data);
    }

    #[test]
    fn test_service_record_update_attr() {
        let mut rec = ServiceRecord::empty();
        rec.add_attr(0x0001, &[0x01]).unwrap();
        rec.add_attr(0x0001, &[0x02]).unwrap(); // update
        let got = rec.get_attr(0x0001).unwrap();
        assert_eq!(got, &[0x02]);
    }

    #[test]
    fn test_service_record_max_attrs() {
        let mut rec = ServiceRecord::empty();
        for i in 0..MAX_ATTRS {
            rec.add_attr(i as u16, &[i as u8]).unwrap();
        }
        // One more should fail
        assert!(rec.add_attr(0xFFFF, &[0]).is_err());
    }

    #[test]
    fn test_sdp_recv_pdu_error_on_unknown() {
        // PDU with unknown ID and some scid — should not panic
        let pdu = [0xFE, 0x00, 0x01, 0x00, 0x00]; // pdu_id=0xFE, tid=1, plen=0
        sdp_recv_pdu(42, &pdu); // scid=42
    }

    #[test]
    fn test_sdp_handles_short_pdu() {
        sdp_recv_pdu(0, &[]);
        sdp_recv_pdu(0, &[0x02, 0x00]);
    }
}
