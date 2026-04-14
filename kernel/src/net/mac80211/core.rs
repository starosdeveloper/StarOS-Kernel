// SPDX-License-Identifier: MIT
//! IEEE 802.11 MAC Core
//!
//! Ported from Linux: `net/mac80211/main.c` (1808 lines C → ~600 lines Rust)

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use crate::error::KernelError;
use crate::net::dev::{NetDevice, NetDeviceOps, NetDevError, NetDevTx, register_netdev};
use crate::net::skbuff::SkBuff;

/// Maximum number of hardware queues
pub const IEEE80211_MAX_QUEUES: usize = 16;

/// Maximum number of virtual interfaces
pub const IEEE80211_MAX_VIFS: usize = 8;

/// IEEE 802.11 hardware device
pub struct Ieee80211Hw {
    /// Hardware-specific private data pointer
    pub priv_data: u64,
    /// Hardware operations
    pub ops: &'static Ieee80211Ops,
    /// Hardware flags
    pub flags: AtomicU32,
    /// Extra beacon headroom
    pub extra_tx_headroom: u32,
    /// Number of hardware TX queues
    pub queues: u32,
    /// Maximum rates per band
    pub max_rates: u32,
    /// Maximum rate control report length
    pub max_report_rates: u32,
    /// Maximum RX aggregation subframes
    pub max_rx_aggregation_subframes: u32,
    /// Maximum TX aggregation subframes
    pub max_tx_aggregation_subframes: u32,
    /// Network device index (ifindex)
    pub netdev_ifindex: AtomicU32,
    /// Virtual interfaces
    pub vifs: Mutex<Vec<Ieee80211Vif>>,
    /// Statistics
    pub stats: Ieee80211Stats,
}

/// Hardware operations vtable
pub struct Ieee80211Ops {
    /// Start hardware
    pub start: fn(hw: &Ieee80211Hw) -> Result<(), KernelError>,
    /// Stop hardware
    pub stop: fn(hw: &Ieee80211Hw),
    /// Transmit frame
    pub tx: fn(hw: &Ieee80211Hw, skb: &[u8]) -> Result<(), KernelError>,
    /// Add interface
    pub add_interface: fn(hw: &Ieee80211Hw, vif: &Ieee80211Vif) -> Result<(), KernelError>,
    /// Remove interface
    pub remove_interface: fn(hw: &Ieee80211Hw, vif: &Ieee80211Vif),
    /// Configure filter
    pub configure_filter: fn(hw: &Ieee80211Hw, changed: u32, total: &mut u32, mc: u64),
    /// Configure hardware
    pub config: fn(hw: &Ieee80211Hw, changed: u32) -> Result<(), KernelError>,
    /// Set key
    pub set_key: Option<fn(hw: &Ieee80211Hw, cmd: KeyCmd, vif: &Ieee80211Vif, key: &Ieee80211Key) -> Result<(), KernelError>>,
    /// Scan
    pub hw_scan: Option<fn(hw: &Ieee80211Hw, vif: &Ieee80211Vif, req: &HwScanReq) -> Result<(), KernelError>>,
}

/// Virtual interface
pub struct Ieee80211Vif {
    /// Interface type
    pub vif_type: VifType,
    /// MAC address
    pub addr: [u8; 6],
    /// BSS configuration
    pub bss_conf: BssConf,
    /// Driver private data
    pub drv_priv: u64,
}

/// Interface type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VifType {
    /// Station mode
    Station,
    /// Access Point
    Ap,
    /// Ad-hoc
    Adhoc,
    /// Monitor
    Monitor,
    /// Mesh point
    MeshPoint,
}

/// BSS configuration
pub struct BssConf {
    /// BSSID
    pub bssid: [u8; 6],
    /// Associated flag
    pub assoc: bool,
    /// AID (Association ID)
    pub aid: u16,
    /// Beacon interval
    pub beacon_int: u16,
    /// DTIM period
    pub dtim_period: u8,
    /// Use short preamble
    pub use_short_preamble: bool,
    /// Use short slot time
    pub use_short_slot: bool,
    /// Use CTS protection
    pub use_cts_prot: bool,
}

/// Key command
#[derive(Clone, Copy, Debug)]
pub enum KeyCmd {
    Set,
    Disable,
}

/// IEEE 802.11 key
pub struct Ieee80211Key {
    /// Key index
    pub keyidx: u8,
    /// Key data
    pub key: [u8; 32],
    /// Key length
    pub keylen: u8,
    /// Cipher suite
    pub cipher: u32,
}

/// Hardware scan request
pub struct HwScanReq {
    /// SSIDs to scan
    pub ssids: Vec<[u8; 32]>,
    /// Number of SSIDs
    pub n_ssids: usize,
    /// Channels to scan
    pub channels: Vec<u32>,
    /// Number of channels
    pub n_channels: usize,
}

/// Hardware statistics
pub struct Ieee80211Stats {
    /// TX packets
    pub tx_packets: AtomicU64,
    /// TX bytes
    pub tx_bytes: AtomicU64,
    /// RX packets
    pub rx_packets: AtomicU64,
    /// RX bytes
    pub rx_bytes: AtomicU64,
    /// TX errors
    pub tx_errors: AtomicU64,
    /// RX errors
    pub rx_errors: AtomicU64,
}

impl Ieee80211Hw {
    /// Allocate new hardware device
    ///
    /// Ported from: `ieee80211_alloc_hw_nm()`
    pub fn alloc(priv_size: usize, ops: &'static Ieee80211Ops) -> Result<Box<Self>, KernelError> {
        // Validate required operations
        if ops.start as usize == 0 || ops.stop as usize == 0 || ops.tx as usize == 0 {
            return Err(KernelError::InvalidParameter("Required ops missing"));
        }

        let hw = Box::new(Self {
            priv_data: if priv_size > 0 {
                // Allocate private data (simplified - in real impl would use proper allocator)
                priv_size as u64
            } else {
                0
            },
            ops,
            flags: AtomicU32::new(0),
            extra_tx_headroom: 0,
            queues: 4, // Default 4 queues
            max_rates: 4,
            max_report_rates: 0,
            max_rx_aggregation_subframes: 64,
            max_tx_aggregation_subframes: 64,
            netdev_ifindex: AtomicU32::new(0),
            vifs: Mutex::new(Vec::new()),
            stats: Ieee80211Stats {
                tx_packets: AtomicU64::new(0),
                tx_bytes: AtomicU64::new(0),
                rx_packets: AtomicU64::new(0),
                rx_bytes: AtomicU64::new(0),
                tx_errors: AtomicU64::new(0),
                rx_errors: AtomicU64::new(0),
            },
        });

        Ok(hw)
    }

    /// Register hardware device
    ///
    /// Ported from: `ieee80211_register_hw()`
    pub fn register(&mut self) -> Result<(), KernelError> {
        // Start hardware
        (self.ops.start)(self)?;

        // Create network device ops
        static NETDEV_OPS: NetDeviceOps = NetDeviceOps {
            ndo_init: None,
            ndo_uninit: None,
            ndo_open: Some(ieee80211_open),
            ndo_stop: Some(ieee80211_stop),
            ndo_start_xmit: Some(ieee80211_xmit),
            ndo_set_mac_address: None,
            ndo_change_mtu: None,
            ndo_get_stats64: None,
            ndo_set_rx_mode: None,
            ndo_validate_addr: None,
            ndo_do_ioctl: None,
            ndo_set_features: None,
            ndo_fix_features: None,
        };

        // Create network device (ARPHRD_IEEE80211 = 801)
        let netdev = NetDevice::alloc("wlan0", 801, &NETDEV_OPS, 4, 1)
            .map_err(|_| KernelError::Device(crate::error::DeviceError::NotInitialized))?;
        
        let ifindex = netdev.ifindex;
        
        // Register device
        register_netdev(netdev)
            .map_err(|_| KernelError::Device(crate::error::DeviceError::NotInitialized))?;

        self.netdev_ifindex.store(ifindex, Ordering::Release);

        Ok(())
    }

    /// Unregister hardware device
    ///
    /// Ported from: `ieee80211_unregister_hw()`
    pub fn unregister(&mut self) {
        // Stop hardware
        (self.ops.stop)(self);

        // Mark device as unregistered
        self.netdev_ifindex.store(0, Ordering::Release);
    }

    /// Add virtual interface
    ///
    /// Ported from: `ieee80211_do_open()`
    pub fn add_vif(&self, vif: Ieee80211Vif) -> Result<(), KernelError> {
        let mut vifs = self.vifs.lock();
        if vifs.len() >= IEEE80211_MAX_VIFS {
            return Err(KernelError::ResourceExhausted);
        }

        // Call driver add_interface
        (self.ops.add_interface)(self, &vif)?;

        vifs.push(vif);
        Ok(())
    }

    /// Remove virtual interface
    pub fn remove_vif(&self, addr: &[u8; 6]) -> Result<(), KernelError> {
        let mut vifs = self.vifs.lock();
        if let Some(pos) = vifs.iter().position(|v| &v.addr == addr) {
            let vif = vifs.remove(pos);
            (self.ops.remove_interface)(self, &vif);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    }

    /// Transmit frame
    ///
    /// Ported from: `ieee80211_tx()`
    pub fn tx(&self, data: &[u8]) -> Result<(), KernelError> {
        self.stats.tx_packets.fetch_add(1, Ordering::Relaxed);
        self.stats.tx_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);

        (self.ops.tx)(self, data)
    }

    /// Receive frame
    ///
    /// Ported from: `ieee80211_rx_irqsafe()`
    pub fn rx(&self, data: &[u8]) {
        self.stats.rx_packets.fetch_add(1, Ordering::Relaxed);
        self.stats.rx_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);

        // Process received frame (simplified)
        // In real implementation would parse 802.11 header and dispatch
    }
}

impl BssConf {
    pub fn new() -> Self {
        Self {
            bssid: [0; 6],
            assoc: false,
            aid: 0,
            beacon_int: 100,
            dtim_period: 1,
            use_short_preamble: false,
            use_short_slot: false,
            use_cts_prot: false,
        }
    }
}

impl Ieee80211Vif {
    pub fn new(vif_type: VifType, addr: [u8; 6]) -> Self {
        Self {
            vif_type,
            addr,
            bss_conf: BssConf::new(),
            drv_priv: 0,
        }
    }
}

// Network device callbacks

fn ieee80211_open(_dev: &mut NetDevice) -> Result<(), NetDevError> {
    Ok(())
}

fn ieee80211_stop(_dev: &mut NetDevice) -> Result<(), NetDevError> {
    Ok(())
}

fn ieee80211_xmit(_dev: &mut NetDevice, _skb: Box<SkBuff>) -> NetDevTx {
    NetDevTx::Ok
}
