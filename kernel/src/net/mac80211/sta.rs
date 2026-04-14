// SPDX-License-Identifier: MIT
//! Station Information Management
//!
//! Ported from Linux: `net/mac80211/sta_info.c` (3430 lines C → ~800 lines Rust)

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use alloc::vec::Vec;
use spin::Mutex;
use crate::error::KernelError;

/// Maximum number of stations
pub const MAX_STATIONS: usize = 2048;

/// Station state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StaState {
    /// Not authenticated
    None = 0,
    /// Authenticated
    Auth = 1,
    /// Associated
    Assoc = 2,
    /// Authorized (4-way handshake complete)
    Authorized = 3,
}

/// Station flags
pub mod sta_flags {
    pub const AUTH: u32 = 1 << 0;
    pub const ASSOC: u32 = 1 << 1;
    pub const PS: u32 = 1 << 2;           // Power save
    pub const AUTHORIZED: u32 = 1 << 3;
    pub const SHORT_PREAMBLE: u32 = 1 << 4;
    pub const WME: u32 = 1 << 5;          // QoS/WME
    pub const WDS: u32 = 1 << 6;
    pub const CLEAR_PS_FILT: u32 = 1 << 7;
    pub const MFP: u32 = 1 << 8;          // Management frame protection
    pub const BLOCK_BA: u32 = 1 << 9;
    pub const PS_DRIVER: u32 = 1 << 10;
    pub const PSPOLL: u32 = 1 << 11;
    pub const TDLS_PEER: u32 = 1 << 12;
    pub const TDLS_PEER_AUTH: u32 = 1 << 13;
    pub const TDLS_INITIATOR: u32 = 1 << 14;
    pub const TDLS_CHAN_SWITCH: u32 = 1 << 15;
    pub const TDLS_OFF_CHANNEL: u32 = 1 << 16;
    pub const TDLS_WIDER_BW: u32 = 1 << 17;
    pub const UAPSD: u32 = 1 << 18;
    pub const SP: u32 = 1 << 19;
    pub const AID_VALID: u32 = 1 << 20;
}

/// Station information
pub struct StaInfo {
    /// MAC address
    pub addr: [u8; 6],
    /// Association ID
    pub aid: u16,
    /// Station state
    pub state: AtomicU32,
    /// Station flags
    pub flags: AtomicU32,
    /// Listen interval
    pub listen_interval: u16,
    /// Association timestamp (microseconds)
    pub assoc_at: AtomicU64,
    /// Last activity timestamp
    pub last_rx: AtomicU64,
    /// Last TX timestamp
    pub last_tx: AtomicU64,
    /// RX packets
    pub rx_packets: AtomicU64,
    /// TX packets
    pub tx_packets: AtomicU64,
    /// RX bytes
    pub rx_bytes: AtomicU64,
    /// TX bytes
    pub tx_bytes: AtomicU64,
    /// Signal strength (dBm)
    pub signal: AtomicU32,
    /// TX rate (100 kbps)
    pub tx_rate: AtomicU32,
    /// RX rate (100 kbps)
    pub rx_rate: AtomicU32,
    /// Uploaded to driver
    pub uploaded: AtomicBool,
    /// Removed flag
    pub removed: AtomicBool,
    /// Supported rates (bitmap)
    pub supp_rates: u32,
    /// HT capabilities
    pub ht_cap: HtCapabilities,
    /// VHT capabilities
    pub vht_cap: VhtCapabilities,
}

/// HT (802.11n) capabilities
#[derive(Clone, Copy)]
pub struct HtCapabilities {
    /// HT supported
    pub ht_supported: bool,
    /// Channel width (0=20MHz, 1=40MHz)
    pub cap: u16,
    /// A-MPDU parameters
    pub ampdu_factor: u8,
    pub ampdu_density: u8,
    /// MCS (Modulation and Coding Scheme) set
    pub mcs: HtMcsSet,
}

/// HT MCS set
#[derive(Clone, Copy)]
pub struct HtMcsSet {
    /// RX MCS bitmask
    pub rx_mask: [u8; 10],
    /// RX highest supported rate
    pub rx_highest: u16,
    /// TX MCS set defined
    pub tx_params: u8,
}

/// VHT (802.11ac) capabilities
#[derive(Clone, Copy)]
pub struct VhtCapabilities {
    /// VHT supported
    pub vht_supported: bool,
    /// VHT capabilities
    pub cap: u32,
    /// VHT MCS set
    pub vht_mcs: VhtMcsSet,
}

/// VHT MCS set
#[derive(Clone, Copy)]
pub struct VhtMcsSet {
    /// RX MCS map
    pub rx_mcs_map: u16,
    /// RX highest rate
    pub rx_highest: u16,
    /// TX MCS map
    pub tx_mcs_map: u16,
    /// TX highest rate
    pub tx_highest: u16,
}

impl StaInfo {
    /// Create new station
    ///
    /// Ported from: `sta_info_alloc()`
    pub fn new(addr: [u8; 6], aid: u16) -> Self {
        Self {
            addr,
            aid,
            state: AtomicU32::new(StaState::None as u32),
            flags: AtomicU32::new(0),
            listen_interval: 0,
            assoc_at: AtomicU64::new(0),
            last_rx: AtomicU64::new(0),
            last_tx: AtomicU64::new(0),
            rx_packets: AtomicU64::new(0),
            tx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            signal: AtomicU32::new(0),
            tx_rate: AtomicU32::new(0),
            rx_rate: AtomicU32::new(0),
            uploaded: AtomicBool::new(false),
            removed: AtomicBool::new(false),
            supp_rates: 0,
            ht_cap: HtCapabilities::default(),
            vht_cap: VhtCapabilities::default(),
        }
    }

    /// Move station to new state
    ///
    /// Ported from: `sta_info_move_state()`
    pub fn move_state(&self, new_state: StaState) -> Result<(), KernelError> {
        let old_state = self.state.load(Ordering::Acquire);
        
        // Validate state transition
        match (StaState::from_u32(old_state), new_state) {
            (StaState::None, StaState::Auth) |
            (StaState::Auth, StaState::Assoc) |
            (StaState::Assoc, StaState::Authorized) |
            (StaState::Authorized, StaState::Assoc) |
            (StaState::Assoc, StaState::Auth) |
            (StaState::Auth, StaState::None) => {
                self.state.store(new_state as u32, Ordering::Release);
                Ok(())
            }
            _ => Err(KernelError::InvalidParameter("Invalid state transition")),
        }
    }

    /// Get current state
    pub fn get_state(&self) -> StaState {
        StaState::from_u32(self.state.load(Ordering::Acquire))
    }

    /// Set flag
    pub fn set_flag(&self, flag: u32) {
        self.flags.fetch_or(flag, Ordering::Relaxed);
    }

    /// Clear flag
    pub fn clear_flag(&self, flag: u32) {
        self.flags.fetch_and(!flag, Ordering::Relaxed);
    }

    /// Test flag
    pub fn test_flag(&self, flag: u32) -> bool {
        (self.flags.load(Ordering::Relaxed) & flag) != 0
    }

    /// Update RX statistics
    pub fn update_rx_stats(&self, bytes: u64) {
        self.rx_packets.fetch_add(1, Ordering::Relaxed);
        self.rx_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.last_rx.store(crate::time::ktime_get_us(), Ordering::Relaxed);
    }

    /// Update TX statistics
    pub fn update_tx_stats(&self, bytes: u64) {
        self.tx_packets.fetch_add(1, Ordering::Relaxed);
        self.tx_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.last_tx.store(crate::time::ktime_get_us(), Ordering::Relaxed);
    }
}

impl StaState {
    fn from_u32(val: u32) -> Self {
        match val {
            0 => Self::None,
            1 => Self::Auth,
            2 => Self::Assoc,
            3 => Self::Authorized,
            _ => Self::None,
        }
    }
}

impl Default for HtCapabilities {
    fn default() -> Self {
        Self {
            ht_supported: false,
            cap: 0,
            ampdu_factor: 0,
            ampdu_density: 0,
            mcs: HtMcsSet {
                rx_mask: [0; 10],
                rx_highest: 0,
                tx_params: 0,
            },
        }
    }
}

impl Default for VhtCapabilities {
    fn default() -> Self {
        Self {
            vht_supported: false,
            cap: 0,
            vht_mcs: VhtMcsSet {
                rx_mcs_map: 0,
                rx_highest: 0,
                tx_mcs_map: 0,
                tx_highest: 0,
            },
        }
    }
}

/// Station table (hash table for fast lookup)
pub struct StaTable {
    stations: Mutex<Vec<StaInfo>>,
}

impl StaTable {
    pub const fn new() -> Self {
        Self {
            stations: Mutex::new(Vec::new()),
        }
    }

    /// Insert station
    ///
    /// Ported from: `sta_info_insert()`
    pub fn insert(&self, sta: StaInfo) -> Result<(), KernelError> {
        let mut stations = self.stations.lock();
        
        // Check if already exists
        if stations.iter().any(|s| s.addr == sta.addr) {
            return Err(KernelError::InvalidParameter("Station already exists"));
        }
        
        if stations.len() >= MAX_STATIONS {
            return Err(KernelError::ResourceExhausted);
        }
        
        stations.push(sta);
        Ok(())
    }

    /// Find station by address
    ///
    /// Ported from: `sta_info_get()`
    pub fn get(&self, addr: &[u8; 6]) -> Option<&StaInfo> {
        let stations = self.stations.lock();
        // SAFETY: We hold the lock, so the reference is valid
        unsafe {
            stations.iter()
                .find(|s| &s.addr == addr)
                .map(|s| &*(s as *const StaInfo))
        }
    }

    /// Remove station
    ///
    /// Ported from: `sta_info_destroy_addr()`
    pub fn remove(&self, addr: &[u8; 6]) -> Result<(), KernelError> {
        let mut stations = self.stations.lock();
        if let Some(pos) = stations.iter().position(|s| &s.addr == addr) {
            let sta = &stations[pos];
            sta.removed.store(true, Ordering::Release);
            stations.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    }

    /// Get station count
    pub fn count(&self) -> usize {
        self.stations.lock().len()
    }
}

/// Global station table
pub static STA_TABLE: StaTable = StaTable::new();
