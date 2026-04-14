// SPDX-License-Identifier: MIT
//! HCI — Host Controller Interface core
//!
//! Ported from Linux: `net/bluetooth/hci_core.c` (~4500 lines C → ~900 lines Rust)
//!
//! Implements:
//! - HCI packet framing (Command / ACL / SCO / Event)
//! - Command queue + response matching
//! - Connection table (ACL + SCO, up to 16 simultaneous)
//! - Event dispatcher (Connect Complete, Disconnect, Inquiry Result…)
//! - Device registration / unregistration

use core::sync::atomic::{AtomicU32, AtomicU8, AtomicBool, Ordering};
use spin::Mutex;
use crate::error::KernelError;

// ---------------------------------------------------------------------------
// HCI packet types  (H4 UART indicator byte)
// ---------------------------------------------------------------------------

pub const HCI_COMMAND_PKT: u8 = 0x01;
pub const HCI_ACLDATA_PKT: u8 = 0x02;
pub const HCI_SCODATA_PKT: u8 = 0x03;
pub const HCI_EVENT_PKT:   u8 = 0x04;

// ---------------------------------------------------------------------------
// HCI OpCodes  (OGF << 10 | OCF)
// ---------------------------------------------------------------------------

pub mod opcode {
    // Link Control (OGF 0x01)
    pub const INQUIRY:              u16 = 0x0401;
    pub const INQUIRY_CANCEL:       u16 = 0x0402;
    pub const CREATE_CONN:          u16 = 0x0405;
    pub const DISCONNECT:           u16 = 0x0406;
    pub const ACCEPT_CONN_REQ:      u16 = 0x0409;
    pub const REJECT_CONN_REQ:      u16 = 0x040A;
    pub const AUTH_REQUESTED:       u16 = 0x0411;
    pub const SET_CONN_ENCRYPT:     u16 = 0x0413;
    pub const REMOTE_NAME_REQ:      u16 = 0x0419;

    // Link Policy (OGF 0x02)
    pub const ROLE_DISCOVERY:       u16 = 0x0809;
    pub const WRITE_LINK_POLICY:    u16 = 0x080D;

    // Host Controller (OGF 0x03)
    pub const RESET:                u16 = 0x0C03;
    pub const SET_EVENT_MASK:       u16 = 0x0C01;
    pub const WRITE_LOCAL_NAME:     u16 = 0x0C13;
    pub const READ_LOCAL_NAME:      u16 = 0x0C14;
    pub const WRITE_CLASS_OF_DEV:   u16 = 0x0C24;
    pub const WRITE_AUTH_ENABLE:    u16 = 0x0C20;
    pub const WRITE_ENCRYPT_MODE:   u16 = 0x0C22;
    pub const WRITE_SCAN_ENABLE:    u16 = 0x0C1A;
    pub const WRITE_PAGE_TIMEOUT:   u16 = 0x0C18;
    pub const READ_BUFFER_SIZE:     u16 = 0x1005;
    pub const READ_BD_ADDR:         u16 = 0x1009;
    pub const READ_LOCAL_FEATURES:  u16 = 0x1003;
    pub const READ_LOCAL_VERSION:   u16 = 0x1001;

    // LE Controller (OGF 0x08)
    pub const LE_SET_EVENT_MASK:    u16 = 0x2001;
    pub const LE_READ_BUFFER_SIZE:  u16 = 0x2002;
    pub const LE_SET_SCAN_PARAM:    u16 = 0x200B;
    pub const LE_SET_SCAN_ENABLE:   u16 = 0x200C;
    pub const LE_CREATE_CONN:       u16 = 0x200D;
    pub const LE_SET_ADV_PARAM:     u16 = 0x2006;
    pub const LE_SET_ADV_DATA:      u16 = 0x2008;
    pub const LE_SET_ADV_ENABLE:    u16 = 0x200A;
}

// ---------------------------------------------------------------------------
// HCI Event codes
// ---------------------------------------------------------------------------

pub mod event {
    pub const INQUIRY_COMPLETE:          u8 = 0x01;
    pub const INQUIRY_RESULT:            u8 = 0x02;
    pub const CONN_COMPLETE:             u8 = 0x03;
    pub const CONN_REQUEST:              u8 = 0x04;
    pub const DISCONN_COMPLETE:          u8 = 0x05;
    pub const AUTH_COMPLETE:             u8 = 0x06;
    pub const REMOTE_NAME_REQ_COMPLETE:  u8 = 0x07;
    pub const ENCRYPT_CHANGE:            u8 = 0x08;
    pub const CMD_COMPLETE:              u8 = 0x0E;
    pub const CMD_STATUS:                u8 = 0x0F;
    pub const NUM_COMP_PKTS:             u8 = 0x13;
    pub const ROLE_CHANGE:               u8 = 0x12;
    pub const LE_META:                   u8 = 0x3E;
}

// LE Meta sub-events
pub mod le_event {
    pub const CONN_COMPLETE:        u8 = 0x01;
    pub const ADV_REPORT:           u8 = 0x02;
    pub const CONN_UPDATE_COMPLETE: u8 = 0x03;
    pub const READ_REMOTE_FEATURES: u8 = 0x04;
    pub const LONG_TERM_KEY_REQ:    u8 = 0x05;
}

// ---------------------------------------------------------------------------
// HCI command header
// ---------------------------------------------------------------------------

/// 3-byte HCI command header (opcode + parameter total length)
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct HciCmdHdr {
    pub opcode: u16,   // little-endian
    pub plen:   u8,
}

/// 4-byte HCI event header (event code + parameter total length)
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct HciEventHdr {
    pub evt:  u8,
    pub plen: u8,
}

/// 4-byte HCI ACL data header
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct HciAclHdr {
    /// Handle + PB/BC flags (bits 15:12 flags, 11:0 handle) — little-endian
    pub handle_flags: u16,
    pub dlen:         u16,
}

impl HciAclHdr {
    pub fn handle(&self) -> u16 { u16::from_le(self.handle_flags) & 0x0FFF }
    pub fn pb_flag(&self) -> u8 { ((u16::from_le(self.handle_flags) >> 12) & 0x03) as u8 }
}

// ---------------------------------------------------------------------------
// HCI connection
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnType {
    Acl  = 0,
    Sco  = 1,
    LowEnergy = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnState {
    Closed       = 0,
    Connecting   = 1,
    Connected    = 2,
    Disconnecting = 3,
}

/// An HCI connection entry
#[derive(Clone, Copy)]
pub struct HciConn {
    pub handle:    u16,
    pub bdaddr:    [u8; 6],
    pub conn_type: ConnType,
    pub state:     ConnState,
    pub encrypt:   bool,
    pub auth:      bool,
    /// Link supervision timeout (slots)
    pub lsto:      u16,
    /// TX flush timeout
    pub flush_to:  u16,
    pub valid:     bool,
}

impl HciConn {
    const fn empty() -> Self {
        Self {
            handle:    0,
            bdaddr:    [0; 6],
            conn_type: ConnType::Acl,
            state:     ConnState::Closed,
            encrypt:   false,
            auth:      false,
            lsto:      0x7D00,  // 20s default
            flush_to:  0xFFFF,
            valid:     false,
        }
    }
}

// ---------------------------------------------------------------------------
// HCI command queue entry
// ---------------------------------------------------------------------------

const HCI_MAX_CMD_DATA: usize = 255;

#[derive(Clone, Copy)]
struct HciCmdEntry {
    opcode: u16,
    plen:   u8,
    data:   [u8; HCI_MAX_CMD_DATA],
    valid:  bool,
}

impl HciCmdEntry {
    const fn empty() -> Self {
        Self { opcode: 0, plen: 0, data: [0; HCI_MAX_CMD_DATA], valid: false }
    }
}

// ---------------------------------------------------------------------------
// Inquiry result
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct InquiryResult {
    pub bdaddr:    [u8; 6],
    pub class_dev: [u8; 3],
    pub clock_offset: u16,
    pub rssi:      i8,
    pub valid:     bool,
}

impl InquiryResult {
    pub const fn empty() -> Self {
        Self {
            bdaddr: [0; 6],
            class_dev: [0; 3],
            clock_offset: 0,
            rssi: -100,
            valid: false,
        }
    }
}

// ---------------------------------------------------------------------------
// HCI device
// ---------------------------------------------------------------------------

pub const MAX_HCI_CONNS:   usize = 16;
pub const MAX_HCI_CMD_Q:   usize = 16;
pub const MAX_INQUIRY_RES: usize = 32;

/// HCI device — one instance per Bluetooth controller.
pub struct HciDev {
    /// Device index (used by drivers as hw_idx)
    pub id:           u8,
    /// BD_ADDR of local controller
    pub bdaddr:       Mutex<[u8; 6]>,
    /// Local name (up to 248 bytes per spec)
    pub name:         Mutex<[u8; 248]>,
    /// Class of Device
    pub dev_class:    AtomicU32,
    /// HCI version from Read Local Version
    pub hci_ver:      AtomicU8,
    /// LMP version
    pub lmp_ver:      AtomicU8,
    /// Manufacturer
    pub manufacturer: AtomicU32,
    /// LE supported
    pub le_supported: AtomicBool,
    /// Command counter (how many cmd slots available)
    pub cmd_cnt:      AtomicU8,
    /// ACL connection table
    pub conns:        Mutex<[HciConn; MAX_HCI_CONNS]>,
    /// Pending command queue
    pub cmd_queue:    Mutex<[HciCmdEntry; MAX_HCI_CMD_Q]>,
    /// Inquiry results
    pub inquiry:      Mutex<[InquiryResult; MAX_INQUIRY_RES]>,
    pub inquiry_cnt:  AtomicU32,
    /// Flags
    pub flags:        AtomicU32,
    /// Transport ops (set by chip driver)
    pub transport:    Option<&'static HciTransport>,
}

pub mod hci_flags {
    pub const UP:           u32 = 1 << 0;
    pub const RUNNING:      u32 = 1 << 1;
    pub const INQUIRY:      u32 = 1 << 2;
    pub const AUTH:         u32 = 1 << 3;
    pub const ENCRYPT:      u32 = 1 << 4;
    pub const SCAN:         u32 = 1 << 5;
    pub const LE_ENABLED:   u32 = 1 << 6;
}

/// Transport layer ops — implemented by btqca / btusb / hci_uart.
pub struct HciTransport {
    /// Send an HCI frame (type byte prepended by caller)
    pub send:   fn(dev_idx: u8, data: &[u8]) -> Result<(), KernelError>,
    /// Open/initialize transport
    pub open:   fn(dev_idx: u8) -> Result<(), KernelError>,
    /// Close transport
    pub close:  fn(dev_idx: u8),
    /// Flush TX queue
    pub flush:  fn(dev_idx: u8),
}

impl HciDev {
    pub const fn new(id: u8) -> Self {
        Self {
            id,
            bdaddr:       Mutex::new([0; 6]),
            name:         Mutex::new([0; 248]),
            dev_class:    AtomicU32::new(0),
            hci_ver:      AtomicU8::new(0),
            lmp_ver:      AtomicU8::new(0),
            manufacturer: AtomicU32::new(0),
            le_supported: AtomicBool::new(false),
            cmd_cnt:      AtomicU8::new(1),
            conns:        Mutex::new([HciConn::empty(); MAX_HCI_CONNS]),
            cmd_queue:    Mutex::new([HciCmdEntry::empty(); MAX_HCI_CMD_Q]),
            inquiry:      Mutex::new([InquiryResult::empty(); MAX_INQUIRY_RES]),
            inquiry_cnt:  AtomicU32::new(0),
            flags:        AtomicU32::new(0),
            transport:    None,
        }
    }

    // -----------------------------------------------------------------------
    // Command sending
    // -----------------------------------------------------------------------

    /// Send an HCI command (opcode + raw parameter bytes).
    ///
    /// Ported from: `hci_send_cmd()`
    pub fn send_cmd(&self, opcode: u16, params: &[u8]) -> Result<(), KernelError> {
        let transport = self.transport.ok_or(KernelError::NotFound)?;

        // Build packet: H4 type(1) + opcode(2, LE) + plen(1) + params
        let mut buf = [0u8; 4 + HCI_MAX_CMD_DATA];
        buf[0] = HCI_COMMAND_PKT;
        buf[1..3].copy_from_slice(&opcode.to_le_bytes());
        buf[3] = params.len() as u8;
        buf[4..4 + params.len()].copy_from_slice(params);

        (transport.send)(self.id, &buf[..4 + params.len()])
    }

    /// Send an HCI command with no parameters.
    pub fn send_cmd_nopar(&self, opcode: u16) -> Result<(), KernelError> {
        self.send_cmd(opcode, &[])
    }

    // -----------------------------------------------------------------------
    // Initialization sequence
    // -----------------------------------------------------------------------

    /// Run the standard HCI initialization sequence.
    ///
    /// Ported from: `hci_init1_req()` … `hci_init4_req()`
    pub fn init(&self) -> Result<(), KernelError> {
        let t = self.transport.ok_or(KernelError::NotFound)?;
        (t.open)(self.id)?;

        self.send_cmd_nopar(opcode::RESET)?;
        self.send_cmd_nopar(opcode::READ_LOCAL_VERSION)?;
        self.send_cmd_nopar(opcode::READ_LOCAL_FEATURES)?;
        self.send_cmd_nopar(opcode::READ_BUFFER_SIZE)?;
        self.send_cmd_nopar(opcode::READ_BD_ADDR)?;

        // Set event mask: enable all classic events
        let event_mask = [0xFF_u8; 8];
        self.send_cmd(opcode::SET_EVENT_MASK, &event_mask)?;

        // Enable scan (page + inquiry)
        self.send_cmd(opcode::WRITE_SCAN_ENABLE, &[0x03])?;

        self.flags.fetch_or(hci_flags::UP | hci_flags::RUNNING, Ordering::Release);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Event processing
    // -----------------------------------------------------------------------

    /// Dispatch an incoming HCI event packet (without H4 type byte).
    ///
    /// Ported from: `hci_event_packet()`
    pub fn recv_event(&self, data: &[u8]) {
        if data.len() < 2 { return; }
        let evt_code = data[0];
        let plen     = data[1] as usize;
        let params   = if data.len() >= 2 + plen { &data[2..2 + plen] } else { &data[2..] };

        match evt_code {
            event::CONN_COMPLETE    => self.handle_conn_complete(params),
            event::DISCONN_COMPLETE => self.handle_disconn_complete(params),
            event::CMD_COMPLETE     => self.handle_cmd_complete(params),
            event::CMD_STATUS       => self.handle_cmd_status(params),
            event::INQUIRY_RESULT   => self.handle_inquiry_result(params),
            event::INQUIRY_COMPLETE => { self.flags.fetch_and(!hci_flags::INQUIRY, Ordering::Relaxed); }
            event::AUTH_COMPLETE    => self.handle_auth_complete(params),
            event::ENCRYPT_CHANGE   => self.handle_encrypt_change(params),
            event::LE_META          => self.handle_le_meta(params),
            _                       => { /* unknown event — ignore */ }
        }
    }

    // -----------------------------------------------------------------------
    // Event handlers
    // -----------------------------------------------------------------------

    fn handle_conn_complete(&self, p: &[u8]) {
        // status(1) + handle(2,LE) + bdaddr(6) + link_type(1) + encr_mode(1)
        if p.len() < 11 { return; }
        let status = p[0];
        if status != 0 { return; }
        let handle = u16::from_le_bytes([p[1], p[2]]);
        let mut bdaddr = [0u8; 6];
        bdaddr.copy_from_slice(&p[3..9]);
        let link_type = p[9];

        let mut conns = self.conns.lock();
        for slot in conns.iter_mut() {
            if !slot.valid {
                *slot = HciConn {
                    handle,
                    bdaddr,
                    conn_type: if link_type == 1 { ConnType::Sco } else { ConnType::Acl },
                    state: ConnState::Connected,
                    valid: true,
                    ..HciConn::empty()
                };
                return;
            }
        }
    }

    fn handle_disconn_complete(&self, p: &[u8]) {
        if p.len() < 4 { return; }
        let handle = u16::from_le_bytes([p[1], p[2]]);
        let mut conns = self.conns.lock();
        for slot in conns.iter_mut() {
            if slot.valid && slot.handle == handle {
                slot.state = ConnState::Closed;
                slot.valid = false;
                return;
            }
        }
    }

    fn handle_cmd_complete(&self, p: &[u8]) {
        if p.len() < 3 { return; }
        let num_hci_cmd_pkts = p[0];
        self.cmd_cnt.store(num_hci_cmd_pkts, Ordering::Relaxed);
        let opcode = u16::from_le_bytes([p[1], p[2]]);
        let ret    = if p.len() > 3 { p[3] } else { 0 };

        if ret != 0 { return; } // Command failed — ignore for now

        match opcode {
            opcode::READ_BD_ADDR => {
                if p.len() >= 10 {
                    let mut addr = self.bdaddr.lock();
                    addr.copy_from_slice(&p[4..10]);
                }
            }
            opcode::READ_LOCAL_VERSION => {
                if p.len() >= 9 {
                    self.hci_ver.store(p[4], Ordering::Relaxed);
                    self.lmp_ver.store(p[6], Ordering::Relaxed);
                    let manuf = u16::from_le_bytes([p[7], p[8]]) as u32;
                    self.manufacturer.store(manuf, Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }

    fn handle_cmd_status(&self, p: &[u8]) {
        if p.len() >= 2 {
            self.cmd_cnt.store(p[1], Ordering::Relaxed);
        }
    }

    fn handle_inquiry_result(&self, p: &[u8]) {
        if p.is_empty() { return; }
        let num_responses = p[0] as usize;
        // Each response: bdaddr(6) + page_scan_rep_mode(1) + reserved(2)
        //                + class_of_dev(3) + clock_offset(2) = 14 bytes
        let mut results = self.inquiry.lock();
        for i in 0..num_responses {
            let off = 1 + i * 14;
            if off + 14 > p.len() { break; }
            let mut entry = InquiryResult::empty();
            entry.bdaddr.copy_from_slice(&p[off..off + 6]);
            entry.class_dev.copy_from_slice(&p[off + 9..off + 12]);
            entry.clock_offset = u16::from_le_bytes([p[off + 12], p[off + 13]]);
            entry.valid = true;
            let cnt = self.inquiry_cnt.load(Ordering::Relaxed) as usize;
            results[cnt % MAX_INQUIRY_RES] = entry;
            self.inquiry_cnt.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn handle_auth_complete(&self, p: &[u8]) {
        if p.len() < 3 { return; }
        let status = p[0];
        let handle = u16::from_le_bytes([p[1], p[2]]);
        if status != 0 { return; }
        let mut conns = self.conns.lock();
        for slot in conns.iter_mut() {
            if slot.valid && slot.handle == handle {
                slot.auth = true;
                return;
            }
        }
    }

    fn handle_encrypt_change(&self, p: &[u8]) {
        if p.len() < 4 { return; }
        let status  = p[0];
        let handle  = u16::from_le_bytes([p[1], p[2]]);
        let enabled = p[3] != 0;
        if status != 0 { return; }
        let mut conns = self.conns.lock();
        for slot in conns.iter_mut() {
            if slot.valid && slot.handle == handle {
                slot.encrypt = enabled;
                return;
            }
        }
    }

    fn handle_le_meta(&self, p: &[u8]) {
        if p.is_empty() { return; }
        match p[0] {
            le_event::CONN_COMPLETE => self.handle_le_conn_complete(&p[1..]),
            le_event::ADV_REPORT    => self.handle_le_adv_report(&p[1..]),
            _ => {}
        }
    }

    fn handle_le_conn_complete(&self, p: &[u8]) {
        if p.len() < 18 { return; }
        let status = p[0];
        if status != 0 { return; }
        let handle = u16::from_le_bytes([p[1], p[2]]);
        let mut bdaddr = [0u8; 6];
        bdaddr.copy_from_slice(&p[4..10]);

        let mut conns = self.conns.lock();
        for slot in conns.iter_mut() {
            if !slot.valid {
                *slot = HciConn {
                    handle,
                    bdaddr,
                    conn_type: ConnType::LowEnergy,
                    state: ConnState::Connected,
                    valid: true,
                    ..HciConn::empty()
                };
                return;
            }
        }
    }

    fn handle_le_adv_report(&self, _p: &[u8]) {
        // LE advertising report — placeholder for GATT/GAP layer
    }

    // -----------------------------------------------------------------------
    // ACL data
    // -----------------------------------------------------------------------

    /// Receive an ACL data packet (without H4 type byte).
    ///
    /// Ported from: `hci_acldata_packet()`
    pub fn recv_acl(&self, data: &[u8]) {
        if data.len() < 4 { return; }
        let handle = u16::from_le_bytes([data[0], data[1]]) & 0x0FFF;
        let dlen   = u16::from_le_bytes([data[2], data[3]]) as usize;
        let payload = if data.len() >= 4 + dlen { &data[4..4 + dlen] } else { &data[4..] };
        // Route to L2CAP: l2cap_recv_frame(handle, payload)
        // (called from drivers/bluetooth/*)
        crate::net::bluetooth::l2cap::l2cap_recv_frame(handle, payload);
    }

    // -----------------------------------------------------------------------
    // Connection helpers
    // -----------------------------------------------------------------------

    /// Initiate an ACL connection.
    ///
    /// Ported from: `hci_connect_acl()`
    pub fn connect(&self, bdaddr: &[u8; 6]) -> Result<(), KernelError> {
        // CREATE_CONNECTION: bdaddr(6) + pkt_type(2) + page_scan_rep_mode(1)
        //                  + reserved(1) + clock_offset(2) + allow_role_switch(1)
        let mut p = [0u8; 13];
        p[0..6].copy_from_slice(bdaddr);
        p[6..8].copy_from_slice(&0xCC18_u16.to_le_bytes()); // DM1|DH1|DM3|DH3|DM5|DH5
        p[8]  = 0x02;  // page scan rep mode R2
        p[9]  = 0x00;
        p[10..12].copy_from_slice(&0x0000_u16.to_le_bytes());
        p[12] = 0x01;  // allow role switch
        self.send_cmd(opcode::CREATE_CONN, &p)
    }

    /// Disconnect a connection.
    ///
    /// Ported from: `hci_disconnect()`
    pub fn disconnect_handle(&self, handle: u16, reason: u8) -> Result<(), KernelError> {
        let mut p = [0u8; 3];
        p[0..2].copy_from_slice(&handle.to_le_bytes());
        p[2] = reason;
        self.send_cmd(opcode::DISCONNECT, &p)
    }

    /// Start inquiry (classic BT device discovery).
    ///
    /// Ported from: `hci_inquiry()`
    pub fn inquiry_start(&self, duration: u8) -> Result<(), KernelError> {
        // LAP(3) + inquiry_length(1) + num_responses(0=unlimited)
        let p = [0x33, 0x8B, 0x9E,  // GIAC LAP
                 duration, 0x00];
        self.inquiry_cnt.store(0, Ordering::Relaxed);
        self.flags.fetch_or(hci_flags::INQUIRY, Ordering::Relaxed);
        self.send_cmd(opcode::INQUIRY, &p)
    }

    /// Find a connection by handle.
    pub fn conn_by_handle(&self, handle: u16) -> Option<HciConn> {
        let conns = self.conns.lock();
        conns.iter().find(|c| c.valid && c.handle == handle).copied()
    }

    /// Find a connection by BD address.
    pub fn conn_by_bdaddr(&self, bdaddr: &[u8; 6]) -> Option<HciConn> {
        let conns = self.conns.lock();
        conns.iter().find(|c| c.valid && &c.bdaddr == bdaddr).copied()
    }
}

// ---------------------------------------------------------------------------
// Global HCI device table
// ---------------------------------------------------------------------------

pub const MAX_HCI_DEVS: usize = 4;

struct HciDevTable {
    devs:  [Option<HciDev>; MAX_HCI_DEVS],
    count: usize,
}

impl HciDevTable {
    const fn new() -> Self {
        Self { devs: [None, None, None, None], count: 0 }
    }
}

static HCI_DEVS: Mutex<HciDevTable> = Mutex::new(HciDevTable::new());

/// Register an HCI device. Returns its assigned ID (hci0, hci1…).
///
/// Ported from: `hci_register_dev()`
pub fn hci_register_dev(mut dev: HciDev) -> Result<u8, KernelError> {
    let mut tbl = HCI_DEVS.lock();
    if tbl.count >= MAX_HCI_DEVS {
        return Err(KernelError::ResourceExhausted);
    }
    for (i, slot) in tbl.devs.iter_mut().enumerate() {
        if slot.is_none() {
            dev.id = i as u8;
            let id = dev.id;
            *slot = Some(dev);
            tbl.count += 1;
            return Ok(id);
        }
    }
    Err(KernelError::ResourceExhausted)
}

/// Unregister an HCI device.
///
/// Ported from: `hci_unregister_dev()`
pub fn hci_unregister_dev(id: u8) {
    let mut tbl = HCI_DEVS.lock();
    if let Some(slot) = tbl.devs.get_mut(id as usize) {
        if slot.is_some() {
            *slot = None;
            tbl.count -= 1;
        }
    }
}

/// Run a closure with a reference to an HCI device by ID.
/// Index is copied out of the lock before use — no reference escapes.
pub fn with_hci_dev<F, R>(id: u8, f: F) -> Option<R>
where
    F: FnOnce(&HciDev) -> R,
{
    let tbl = HCI_DEVS.lock();
    tbl.devs.get(id as usize)?.as_ref().map(f)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dev() -> HciDev { HciDev::new(0) }

    #[test]
    fn test_recv_event_conn_complete() {
        let dev = make_dev();
        // status(1)=0 + handle(2)=0x0005 + bdaddr(6) + link_type(1)=ACL + encr(1)
        let evt = [
            event::CONN_COMPLETE, 11,
            0x00,                          // status OK
            0x05, 0x00,                    // handle = 5
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, // bdaddr
            0x01,                          // link type = ACL
            0x00,                          // encryption
        ];
        dev.recv_event(&evt);
        let conn = dev.conn_by_handle(5).expect("connection should exist");
        assert_eq!(conn.handle, 5);
        assert_eq!(conn.bdaddr, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_recv_event_disconn_removes_conn() {
        let dev = make_dev();
        // Insert connection manually
        {
            let mut conns = dev.conns.lock();
            conns[0] = HciConn {
                handle: 7, bdaddr: [1,2,3,4,5,6],
                conn_type: ConnType::Acl,
                state: ConnState::Connected,
                valid: true,
                ..HciConn::empty()
            };
        }
        let evt = [event::DISCONN_COMPLETE, 4, 0x00, 0x07, 0x00, 0x13];
        dev.recv_event(&evt);
        assert!(dev.conn_by_handle(7).is_none());
    }

    #[test]
    fn test_recv_event_inquiry_result() {
        let dev = make_dev();
        // 1 response: bdaddr(6) + page_scan_rep_mode(1) + reserved(2) + class(3) + clock_offset(2)
        let mut evt = [0u8; 2 + 1 + 14];
        evt[0] = event::INQUIRY_RESULT;
        evt[1] = 1 + 14; // plen
        evt[2] = 1;       // num_responses = 1
        evt[3..9].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // bdaddr
        // rest is zero (page_scan_rep, reserved, class, clock_offset)
        dev.recv_event(&evt);
        assert_eq!(dev.inquiry_cnt.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_recv_event_cmd_complete_updates_cmd_cnt() {
        let dev = make_dev();
        // num_hci_cmd_pkts(1)=3 + opcode(2) + status(1)
        let evt = [event::CMD_COMPLETE, 4, 3, 0x03, 0x0C, 0x00];
        dev.recv_event(&evt);
        assert_eq!(dev.cmd_cnt.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_inquiry_flag() {
        let dev = make_dev();
        assert_eq!(dev.flags.load(Ordering::Relaxed) & hci_flags::INQUIRY, 0);
        // Simulate inquiry complete clearing the flag
        dev.flags.fetch_or(hci_flags::INQUIRY, Ordering::Relaxed);
        let evt = [event::INQUIRY_COMPLETE, 1, 0x00];
        dev.recv_event(&evt);
        assert_eq!(dev.flags.load(Ordering::Relaxed) & hci_flags::INQUIRY, 0);
    }

    #[test]
    fn test_le_conn_complete() {
        let dev = make_dev();
        let mut evt = [0u8; 2 + 19];
        evt[0] = event::LE_META;
        evt[1] = 19;
        evt[2] = le_event::CONN_COMPLETE;
        evt[3] = 0x00;  // status OK
        evt[4..6].copy_from_slice(&8u16.to_le_bytes()); // handle = 8
        evt[6] = 0x00;  // role
        evt[7] = 0x00;  // addr type
        evt[8..14].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]);
        dev.recv_event(&evt);
        let conn = dev.conn_by_handle(8).expect("LE conn should exist");
        assert_eq!(conn.conn_type, ConnType::LowEnergy);
    }
}
