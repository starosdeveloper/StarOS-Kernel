// SPDX-License-Identifier: MIT
//! WWAN Device Registry
//!
//! Ported from Linux: `drivers/net/wwan/wwan_core.c`
//!
//! Chip-agnostic WWAN device abstraction + global registry.
//! Uses index-based HW table (no raw pointer escape from mutex lock).

use spin::Mutex;
use crate::error::KernelError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_WWAN_DEVS: usize = 4;

// ---------------------------------------------------------------------------
// Data connection state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WwanState {
    Uninitialized,
    Registered,      // Attached to network
    Connecting,
    Connected,
    Disconnected,
    Error,
}

// ---------------------------------------------------------------------------
// Radio access technology
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioTech {
    Unknown,
    Gsm,
    Umts,
    Lte,
    NrSa,
    NrNsa,
}

// ---------------------------------------------------------------------------
// Signal quality
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SignalInfo {
    pub rssi_dbm: i16,   // RSSI in dBm (e.g. -80)
    pub rsrp_dbm: i16,   // LTE RSRP in dBm
    pub sinr_db:  i8,    // SINR in dB
    pub rat:      RadioTech,
}

impl SignalInfo {
    pub const fn zero() -> Self {
        Self { rssi_dbm: -120, rsrp_dbm: -140, sinr_db: -20, rat: RadioTech::Unknown }
    }
}

// ---------------------------------------------------------------------------
// Operator info
// ---------------------------------------------------------------------------

pub const OPERATOR_NAME_LEN: usize = 32;
pub const APN_LEN: usize = 64;

#[derive(Clone, Copy)]
pub struct OperatorInfo {
    pub mcc:  u16,
    pub mnc:  u16,
    pub name: [u8; OPERATOR_NAME_LEN],
    pub name_len: u8,
}

impl OperatorInfo {
    pub const fn empty() -> Self {
        Self { mcc: 0, mnc: 0, name: [0u8; OPERATOR_NAME_LEN], name_len: 0 }
    }
    pub fn name_str(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

// ---------------------------------------------------------------------------
// Data bearer / PDP context
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DataBearer {
    pub active:     bool,
    pub pkt_handle: u32,
    pub apn:        [u8; APN_LEN],
    pub apn_len:    u8,
    pub ipv4_addr:  u32,
    pub dns_primary: u32,
    pub dns_secondary: u32,
}

impl DataBearer {
    pub const fn empty() -> Self {
        Self {
            active: false,
            pkt_handle: 0,
            apn: [0u8; APN_LEN],
            apn_len: 0,
            ipv4_addr: 0,
            dns_primary: 0,
            dns_secondary: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// WwanOps vtable (chip-agnostic interface)
// ---------------------------------------------------------------------------

pub struct WwanOps {
    /// Power on the modem
    pub power_on:    fn(hw_idx: u8) -> Result<(), KernelError>,
    /// Power off the modem
    pub power_off:   fn(hw_idx: u8) -> Result<(), KernelError>,
    /// Start a data bearer (establish PDP context)
    pub connect:     fn(hw_idx: u8, apn: &[u8]) -> Result<u32, KernelError>,
    /// Tear down a data bearer
    pub disconnect:  fn(hw_idx: u8, handle: u32) -> Result<(), KernelError>,
    /// Get current signal info
    pub get_signal:  fn(hw_idx: u8) -> SignalInfo,
    /// Get operator info
    pub get_operator: fn(hw_idx: u8) -> OperatorInfo,
}

// ---------------------------------------------------------------------------
// WWAN device
// ---------------------------------------------------------------------------

pub struct WwanDev {
    pub hw_idx:    u8,
    pub ops:       &'static WwanOps,
    pub state:     WwanState,
    pub bearer:    DataBearer,
    pub operator:  OperatorInfo,
    pub signal:    SignalInfo,
    pub active:    bool,
}

impl WwanDev {
    pub const fn new(hw_idx: u8, ops: &'static WwanOps) -> Self {
        Self {
            hw_idx,
            ops,
            state: WwanState::Uninitialized,
            bearer: DataBearer {
                active: false,
                pkt_handle: 0,
                apn: [0u8; APN_LEN],
                apn_len: 0,
                ipv4_addr: 0,
                dns_primary: 0,
                dns_secondary: 0,
            },
            operator: OperatorInfo { mcc: 0, mnc: 0, name: [0u8; OPERATOR_NAME_LEN], name_len: 0 },
            signal: SignalInfo { rssi_dbm: -120, rsrp_dbm: -140, sinr_db: -20, rat: RadioTech::Unknown },
            active: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global registry
// ---------------------------------------------------------------------------

pub struct WwanDevTable {
    pub devs:  [Option<WwanDev>; MAX_WWAN_DEVS],
    pub count: usize,
}

impl WwanDevTable {
    pub const fn new() -> Self {
        Self {
            devs:  [None, None, None, None],
            count: 0,
        }
    }
}

pub static WWAN_DEVS: Mutex<WwanDevTable> = Mutex::new(WwanDevTable::new());

/// Register a new WWAN device and power it on.
pub fn wwan_register(dev: WwanDev) -> Result<u8, KernelError> {
    let mut tbl = WWAN_DEVS.lock();
    if tbl.count >= MAX_WWAN_DEVS {
        return Err(KernelError::ResourceExhausted);
    }
    let idx = tbl.count;
    tbl.devs[idx] = Some(dev);
    tbl.count += 1;
    // Power-on outside lock using hw_idx
    let hw_idx = tbl.devs[idx].as_ref().unwrap().hw_idx;
    let ops    = tbl.devs[idx].as_ref().unwrap().ops;
    drop(tbl);
    (ops.power_on)(hw_idx)?;
    // Set registered state
    let mut tbl = WWAN_DEVS.lock();
    if let Some(d) = &mut tbl.devs[idx] {
        d.state = WwanState::Registered;
    }
    Ok(idx as u8)
}

/// Connect a WWAN device to a data bearer with the given APN.
pub fn wwan_connect(dev_slot: u8, apn: &[u8]) -> Result<(), KernelError> {
    if apn.len() > APN_LEN { return Err(KernelError::InvalidParameter("")); }
    let (hw_idx, ops) = {
        let tbl = WWAN_DEVS.lock();
        let idx = dev_slot as usize;
        if idx >= tbl.count { return Err(KernelError::InvalidParameter("")); }
        let d = tbl.devs[idx].as_ref().ok_or(KernelError::InvalidParameter(""))?;
        (d.hw_idx, d.ops)
    };
    let handle = (ops.connect)(hw_idx, apn)?;
    let mut tbl = WWAN_DEVS.lock();
    let idx = dev_slot as usize;
    if let Some(d) = &mut tbl.devs[idx] {
        d.state = WwanState::Connected;
        d.bearer.active = true;
        d.bearer.pkt_handle = handle;
        d.bearer.apn_len = apn.len() as u8;
        d.bearer.apn[..apn.len()].copy_from_slice(apn);
    }
    Ok(())
}

/// Disconnect a WWAN device's data bearer.
pub fn wwan_disconnect(dev_slot: u8) -> Result<(), KernelError> {
    let (hw_idx, ops, handle) = {
        let tbl = WWAN_DEVS.lock();
        let idx = dev_slot as usize;
        if idx >= tbl.count { return Err(KernelError::InvalidParameter("")); }
        let d = tbl.devs[idx].as_ref().ok_or(KernelError::InvalidParameter(""))?;
        (d.hw_idx, d.ops, d.bearer.pkt_handle)
    };
    (ops.disconnect)(hw_idx, handle)?;
    let mut tbl = WWAN_DEVS.lock();
    if let Some(d) = &mut tbl.devs[dev_slot as usize] {
        d.state = WwanState::Disconnected;
        d.bearer.active = false;
    }
    Ok(())
}

/// Safe accessor — runs closure with reference to dev, no pointer escapes lock.
pub fn with_wwan_dev<F, R>(slot: u8, f: F) -> Option<R>
where
    F: FnOnce(&WwanDev) -> R,
{
    let tbl = WWAN_DEVS.lock();
    tbl.devs[slot as usize].as_ref().map(f)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_ops() -> &'static WwanOps {
        static OPS: WwanOps = WwanOps {
            power_on:     |_| Ok(()),
            power_off:    |_| Ok(()),
            connect:      |_, _| Ok(0xDEAD_BEEF),
            disconnect:   |_, _| Ok(()),
            get_signal:   |_| SignalInfo::zero(),
            get_operator: |_| OperatorInfo::empty(),
        };
        &OPS
    }

    #[test]
    fn test_wwan_initial_state() {
        let dev = WwanDev::new(5, dummy_ops());
        assert_eq!(dev.state, WwanState::Uninitialized);
        assert!(!dev.active);
    }

    #[test]
    fn test_wwan_connect_updates_bearer() {
        // Use a fresh slot by accessing table directly to avoid global pollution
        {
            let mut tbl = WWAN_DEVS.lock();
            let idx = tbl.count;
            if idx < MAX_WWAN_DEVS {
                tbl.devs[idx] = Some(WwanDev {
                    hw_idx: 99,
                    ops: dummy_ops(),
                    state: WwanState::Registered,
                    bearer: DataBearer::empty(),
                    operator: OperatorInfo::empty(),
                    signal: SignalInfo::zero(),
                    active: false,
                });
                tbl.count += 1;
            }
        }
        // Find the slot we just inserted
        let slot = {
            let tbl = WWAN_DEVS.lock();
            let mut s = 0u8;
            for i in 0..tbl.count {
                if let Some(d) = &tbl.devs[i] {
                    if d.hw_idx == 99 { s = i as u8; break; }
                }
            }
            s
        };
        assert!(wwan_connect(slot, b"internet").is_ok());
        with_wwan_dev(slot, |d| {
            assert_eq!(d.state, WwanState::Connected);
            assert!(d.bearer.active);
            assert_eq!(d.bearer.pkt_handle, 0xDEAD_BEEF);
        });
    }

    #[test]
    fn test_signal_info_zero() {
        let s = SignalInfo::zero();
        assert_eq!(s.rat, RadioTech::Unknown);
        assert!(s.rssi_dbm <= -100);
    }

    #[test]
    fn test_operator_name_empty() {
        let op = OperatorInfo::empty();
        assert_eq!(op.name_str().len(), 0);
        assert_eq!(op.mcc, 0);
    }
}
