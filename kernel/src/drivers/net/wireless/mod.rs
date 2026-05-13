// SPDX-License-Identifier: MIT
//! Wireless LAN driver infrastructure
//!
//! Ported from Linux: `drivers/net/wireless/` common layer
//!
//! Provides the hardware-agnostic interface that chip-specific drivers
//! (wcn36xx, ath10k, mt76, brcmfmac) implement, connecting to the
//! mac80211 softmac stack above.
//!
//! Architecture:
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │  net::mac80211 (software MAC layer)                      │
//! ├──────────────────────────────────────────────────────────┤
//! │  WirelessOps vtable  (this module)                       │
//! ├──────────┬───────────┬────────────┬───────────────────────┤
//! │ wcn36xx  │  ath10k   │   mt76     │  brcmfmac            │
//! └──────────┴───────────┴────────────┴───────────────────────┘
//! ```

use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use crate::error::KernelError;

pub mod wcn36xx;
pub mod ath10k;
pub mod mt76;
pub mod brcmfmac;

// ---------------------------------------------------------------------------
// Common types
// ---------------------------------------------------------------------------

/// Maximum SSID length (IEEE 802.11 §9.4.2.2)
pub const SSID_MAX_LEN: usize = 32;
/// Maximum number of scan results stored
pub const MAX_SCAN_RESULTS: usize = 32;
/// Maximum supported rates per band
pub const MAX_RATES: usize = 32;
/// Maximum number of channels per band
pub const MAX_CHANNELS: usize = 64;

/// 2.4 GHz / 5 GHz band selector
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Band {
    Band2GHz = 0,
    Band5GHz = 1,
    Band6GHz = 2,
}

/// A single 802.11 channel
#[derive(Clone, Copy, Debug)]
pub struct Channel {
    /// Center frequency in MHz
    pub center_freq: u32,
    /// Channel number
    pub hw_value: u16,
    /// Maximum transmit power (dBm × 100)
    pub max_power: i16,
    /// Channel flags
    pub flags: u32,
    /// Band
    pub band: Band,
}

pub mod channel_flags {
    pub const DISABLED:    u32 = 1 << 0;
    pub const NO_IR:       u32 = 1 << 1;  // No initiating radiation
    pub const RADAR:       u32 = 1 << 3;  // DFS channel
    pub const INDOOR_ONLY: u32 = 1 << 9;
}

/// Scan result entry
#[derive(Clone, Copy)]
pub struct ScanResult {
    /// BSSID
    pub bssid:       [u8; 6],
    /// SSID bytes
    pub ssid:        [u8; SSID_MAX_LEN],
    /// SSID length
    pub ssid_len:    u8,
    /// Signal strength in dBm (signed)
    pub signal_dbm:  i8,
    /// Beacon interval (TUs)
    pub beacon_int:  u16,
    /// Capability information
    pub capability:  u16,
    /// Channel
    pub channel:     Channel,
    /// Timestamp from beacon (TSF)
    pub tsf:         u64,
    /// Entry valid
    pub valid:       bool,
}

impl ScanResult {
    pub const fn empty() -> Self {
        Self {
            bssid: [0; 6],
            ssid: [0; SSID_MAX_LEN],
            ssid_len: 0,
            signal_dbm: -100,
            beacon_int: 100,
            capability: 0,
            channel: Channel {
                center_freq: 0,
                hw_value: 0,
                max_power: 0,
                flags: 0,
                band: Band::Band2GHz,
            },
            tsf: 0,
            valid: false,
        }
    }

    pub fn ssid_str(&self) -> &str {
        core::str::from_utf8(&self.ssid[..self.ssid_len as usize]).unwrap_or("")
    }

    pub fn has_privacy(&self) -> bool {
        (self.capability & 0x0010) != 0
    }
}

/// Wireless driver state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WifiDriverState {
    Uninitialized = 0,
    Initialized,
    Scanning,
    Connecting,
    Connected,
    Error,
}

// ---------------------------------------------------------------------------
// WirelessOps — vtable implemented by each chip driver
// ---------------------------------------------------------------------------

/// Operations that each WiFi chip driver must implement.
///
/// Ported from: `struct ieee80211_ops` + driver-specific probe functions.
pub struct WirelessOps {
    /// Initialize hardware from reset
    pub init:       fn(dev: &WirelessDev) -> Result<(), KernelError>,
    /// Shut down hardware
    pub deinit:     fn(dev: &WirelessDev),
    /// Start a channel scan (non-blocking; results delivered via scan_result_cb)
    pub scan:       fn(dev: &WirelessDev, ssid: Option<&[u8]>) -> Result<(), KernelError>,
    /// Abort an ongoing scan
    pub abort_scan: fn(dev: &WirelessDev),
    /// Connect to a BSS (open or WPA2-PSK)
    pub connect:    fn(dev: &WirelessDev, req: &ConnectReq) -> Result<(), KernelError>,
    /// Disconnect from current BSS
    pub disconnect: fn(dev: &WirelessDev, reason: u16),
    /// Transmit a frame
    pub tx:         fn(dev: &WirelessDev, data: &[u8]) -> Result<(), KernelError>,
    /// Set transmit power (dBm)
    pub set_tx_power: fn(dev: &WirelessDev, dbm: i8) -> Result<(), KernelError>,
    /// Get current signal strength (dBm)
    pub get_signal: fn(dev: &WirelessDev) -> i8,
}

/// Connection request parameters
#[derive(Clone, Copy)]
pub struct ConnectReq {
    /// Target BSSID (or all-zeros for any)
    pub bssid:     [u8; 6],
    /// SSID
    pub ssid:      [u8; SSID_MAX_LEN],
    pub ssid_len:  u8,
    /// Authentication type
    pub auth_type: AuthType,
    /// WPA2 PSK (32-byte PMK derived from passphrase + SSID via PBKDF2-SHA1)
    pub pmk:       [u8; 32],
    /// Channel hint (0 = any)
    pub channel:   u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthType {
    Open,
    Wpa2Psk,
    Wpa3Sae,
}

// ---------------------------------------------------------------------------
// WirelessDev — generic wireless device (index-based, no raw pointers)
// ---------------------------------------------------------------------------

/// Index into `WIRELESS_DEV_TABLE`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WirelessDevIdx(pub u8);

/// Maximum registered wireless devices
pub const MAX_WIRELESS_DEVS: usize = 8;

/// Generic wireless device (HW-independent).
///
/// `hw_idx` is an opaque index into a chip-specific table (no raw pointer).
/// Each chip driver keeps its own `Mutex<[ChipHw; N]>` and uses `hw_idx`
/// to retrieve a mutable reference within a short critical section.
pub struct WirelessDev {
    /// Chip-specific HW table index
    pub hw_idx:     u8,
    /// Driver operations
    pub ops:        &'static WirelessOps,
    /// Driver state
    pub state:      AtomicU32,
    /// Interface flags
    pub flags:      AtomicU32,
    /// MAC address
    pub mac_addr:   [u8; 6],
    /// Scan results ring buffer
    pub scan_results: Mutex<[ScanResult; MAX_SCAN_RESULTS]>,
    /// Number of valid scan results
    pub scan_count: AtomicU32,
    /// Connected BSSID (valid when state == Connected)
    pub bssid:      Mutex<[u8; 6]>,
    /// TX statistics
    pub tx_packets: AtomicU32,
    pub tx_bytes:   AtomicU32,
    pub rx_packets: AtomicU32,
    pub rx_bytes:   AtomicU32,
}

impl WirelessDev {
    pub const fn new(hw_idx: u8, ops: &'static WirelessOps, mac: [u8; 6]) -> Self {
        Self {
            hw_idx,
            ops,
            state:        AtomicU32::new(WifiDriverState::Uninitialized as u32),
            flags:        AtomicU32::new(0),
            mac_addr:     mac,
            scan_results: Mutex::new([ScanResult::empty(); MAX_SCAN_RESULTS]),
            scan_count:   AtomicU32::new(0),
            bssid:        Mutex::new([0; 6]),
            tx_packets:   AtomicU32::new(0),
            tx_bytes:     AtomicU32::new(0),
            rx_packets:   AtomicU32::new(0),
            rx_bytes:     AtomicU32::new(0),
        }
    }

    pub fn get_state(&self) -> WifiDriverState {
        match self.state.load(Ordering::Acquire) {
            0 => WifiDriverState::Uninitialized,
            1 => WifiDriverState::Initialized,
            2 => WifiDriverState::Scanning,
            3 => WifiDriverState::Connecting,
            4 => WifiDriverState::Connected,
            _ => WifiDriverState::Error,
        }
    }

    pub fn set_state(&self, s: WifiDriverState) {
        self.state.store(s as u32, Ordering::Release);
    }

    /// Add a scan result (called from IRQ context by driver).
    pub fn add_scan_result(&self, result: ScanResult) {
        let mut results = self.scan_results.lock();
        let count = self.scan_count.load(Ordering::Relaxed) as usize;
        let idx = count % MAX_SCAN_RESULTS;
        results[idx] = result;
        self.scan_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Initialize hardware.
    pub fn init(&self) -> Result<(), KernelError> {
        (self.ops.init)(self)?;
        self.set_state(WifiDriverState::Initialized);
        Ok(())
    }

    /// Scan for networks.
    pub fn scan(&self, ssid: Option<&[u8]>) -> Result<(), KernelError> {
        if self.get_state() == WifiDriverState::Uninitialized {
            return Err(KernelError::Device(crate::error::DeviceError::NotInitialized));
        }
        self.set_state(WifiDriverState::Scanning);
        (self.ops.scan)(self, ssid)
    }

    /// Connect to network.
    pub fn connect(&self, req: &ConnectReq) -> Result<(), KernelError> {
        self.set_state(WifiDriverState::Connecting);
        (self.ops.connect)(self, req)
    }

    /// Disconnect.
    pub fn disconnect(&self, reason: u16) {
        (self.ops.disconnect)(self, reason);
        self.set_state(WifiDriverState::Initialized);
        *self.bssid.lock() = [0; 6];
    }

    /// Transmit a raw frame.
    pub fn tx(&self, data: &[u8]) -> Result<(), KernelError> {
        let r = (self.ops.tx)(self, data);
        if r.is_ok() {
            self.tx_packets.fetch_add(1, Ordering::Relaxed);
            self.tx_bytes.fetch_add(data.len() as u32, Ordering::Relaxed);
        }
        r
    }
}

// ---------------------------------------------------------------------------
// Global device table
// ---------------------------------------------------------------------------

/// Global wireless device registry (index-based, no heap).
struct WirelessDevTable {
    entries: [Option<WirelessDev>; MAX_WIRELESS_DEVS],
    count:   usize,
}

impl WirelessDevTable {
    const fn new() -> Self {
        // Can't use [None; N] because WirelessDev is not Copy.
        // Use const unsafe workaround: initialize all slots to None via array.
        Self {
            entries: [
                None, None, None, None,
                None, None, None, None,
            ],
            count: 0,
        }
    }
}

static WIRELESS_DEVS: Mutex<WirelessDevTable> = Mutex::new(WirelessDevTable::new());

/// Register a wireless device and return its index.
pub fn register_wireless_dev(dev: WirelessDev) -> Result<WirelessDevIdx, KernelError> {
    let mut tbl = WIRELESS_DEVS.lock();
    if tbl.count >= MAX_WIRELESS_DEVS {
        return Err(KernelError::ResourceExhausted);
    }
    for (i, slot) in tbl.entries.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(dev);
            tbl.count += 1;
            return Ok(WirelessDevIdx(i as u8));
        }
    }
    Err(KernelError::ResourceExhausted)
}

/// Get number of registered devices.
pub fn wireless_dev_count() -> usize {
    WIRELESS_DEVS.lock().count
}
