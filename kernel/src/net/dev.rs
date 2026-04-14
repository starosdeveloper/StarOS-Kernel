// SPDX-License-Identifier: MIT OR Apache-2.0
//! Network Device Layer
//!
//! Ported from Linux: `net/core/dev.c`, `include/linux/netdevice.h`
//!
//! Handles network device registration, packet transmission/reception,
//! and device management.

use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::string::String;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use core::ptr::NonNull;
use spin::Mutex;
use super::skbuff::SkBuff;

/// Maximum interface name size
pub const IFNAMSIZ: usize = 16;

/// Maximum hardware address length
pub const MAX_ADDR_LEN: usize = 32;

/// Network device flags
pub mod net_device_flags {
    pub const IFF_UP: u32 = 1 << 0;           // Interface is up
    pub const IFF_BROADCAST: u32 = 1 << 1;    // Broadcast address valid
    pub const IFF_DEBUG: u32 = 1 << 2;        // Debugging
    pub const IFF_LOOPBACK: u32 = 1 << 3;     // Loopback device
    pub const IFF_POINTOPOINT: u32 = 1 << 4;  // Point-to-point link
    pub const IFF_NOTRAILERS: u32 = 1 << 5;   // Avoid use of trailers
    pub const IFF_RUNNING: u32 = 1 << 6;      // Resources allocated
    pub const IFF_NOARP: u32 = 1 << 7;        // No ARP protocol
    pub const IFF_PROMISC: u32 = 1 << 8;      // Promiscuous mode
    pub const IFF_ALLMULTI: u32 = 1 << 9;     // Receive all multicast
    pub const IFF_MASTER: u32 = 1 << 10;      // Master of load balancer
    pub const IFF_SLAVE: u32 = 1 << 11;       // Slave of load balancer
    pub const IFF_MULTICAST: u32 = 1 << 12;   // Supports multicast
    pub const IFF_PORTSEL: u32 = 1 << 13;     // Can set media type
    pub const IFF_AUTOMEDIA: u32 = 1 << 14;   // Auto media select active
    pub const IFF_DYNAMIC: u32 = 1 << 15;     // Dialup device
    pub const IFF_LOWER_UP: u32 = 1 << 16;    // Driver signals L1 up
    pub const IFF_DORMANT: u32 = 1 << 17;     // Driver signals dormant
    pub const IFF_ECHO: u32 = 1 << 18;        // Echo sent packets
}

/// Network device features
pub type NetdevFeatures = u64;

pub mod netdev_features {
    use super::NetdevFeatures;
    
    pub const NETIF_F_SG: NetdevFeatures = 1 << 0;              // Scatter/gather IO
    pub const NETIF_F_IP_CSUM: NetdevFeatures = 1 << 1;         // IP checksum offload
    pub const NETIF_F_HW_CSUM: NetdevFeatures = 1 << 3;         // Hardware checksum
    pub const NETIF_F_IPV6_CSUM: NetdevFeatures = 1 << 4;       // IPv6 checksum offload
    pub const NETIF_F_HIGHDMA: NetdevFeatures = 1 << 5;         // High DMA
    pub const NETIF_F_FRAGLIST: NetdevFeatures = 1 << 6;        // Scatter/gather IO
    pub const NETIF_F_HW_VLAN_CTAG_TX: NetdevFeatures = 1 << 7; // VLAN TX offload
    pub const NETIF_F_HW_VLAN_CTAG_RX: NetdevFeatures = 1 << 8; // VLAN RX offload
    pub const NETIF_F_TSO: NetdevFeatures = 1 << 11;            // TCP segmentation offload
    pub const NETIF_F_UFO: NetdevFeatures = 1 << 13;            // UDP fragmentation offload
    pub const NETIF_F_GSO: NetdevFeatures = 1 << 14;            // Generic segmentation offload
    pub const NETIF_F_GRO: NetdevFeatures = 1 << 15;            // Generic receive offload
    pub const NETIF_F_LRO: NetdevFeatures = 1 << 16;            // Large receive offload
    pub const NETIF_F_RXCSUM: NetdevFeatures = 1 << 29;         // RX checksum offload
}

/// Network device state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NetDevState {
    /// Device is being initialized
    Init = 0,
    /// Device is registered
    Registered = 1,
    /// Device is going down
    GoingDown = 2,
    /// Device is unregistering
    Unregistering = 3,
    /// Device is unregistered
    Unregistered = 4,
}

/// Network device operations
pub struct NetDeviceOps {
    /// Initialize the device
    pub ndo_init: Option<fn(&mut NetDevice) -> Result<(), NetDevError>>,
    /// Uninitialize the device
    pub ndo_uninit: Option<fn(&mut NetDevice)>,
    /// Open the device
    pub ndo_open: Option<fn(&mut NetDevice) -> Result<(), NetDevError>>,
    /// Stop the device
    pub ndo_stop: Option<fn(&mut NetDevice) -> Result<(), NetDevError>>,
    /// Start transmitting a packet
    pub ndo_start_xmit: Option<fn(&mut NetDevice, skb: Box<SkBuff>) -> NetDevTx>,
    /// Set MAC address
    pub ndo_set_mac_address: Option<fn(&mut NetDevice, addr: &[u8]) -> Result<(), NetDevError>>,
    /// Change MTU
    pub ndo_change_mtu: Option<fn(&mut NetDevice, new_mtu: u32) -> Result<(), NetDevError>>,
    /// Get device statistics
    pub ndo_get_stats64: Option<fn(&NetDevice) -> NetDeviceStats>,
    /// Set RX mode (promiscuous, multicast, etc)
    pub ndo_set_rx_mode: Option<fn(&mut NetDevice)>,
    /// Validate address
    pub ndo_validate_addr: Option<fn(&NetDevice) -> Result<(), NetDevError>>,
    /// Do ioctl
    pub ndo_do_ioctl: Option<fn(&mut NetDevice, cmd: u32, arg: u64) -> Result<i32, NetDevError>>,
    /// Set features
    pub ndo_set_features: Option<fn(&mut NetDevice, features: NetdevFeatures) -> Result<(), NetDevError>>,
    /// Fix features
    pub ndo_fix_features: Option<fn(&NetDevice, features: NetdevFeatures) -> NetdevFeatures>,
}

impl NetDeviceOps {
    pub const fn empty() -> Self {
        Self {
            ndo_init: None,
            ndo_uninit: None,
            ndo_open: None,
            ndo_stop: None,
            ndo_start_xmit: None,
            ndo_set_mac_address: None,
            ndo_change_mtu: None,
            ndo_get_stats64: None,
            ndo_set_rx_mode: None,
            ndo_validate_addr: None,
            ndo_do_ioctl: None,
            ndo_set_features: None,
            ndo_fix_features: None,
        }
    }
}

/// Transmit return codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum NetDevTx {
    /// Driver took care of packet
    Ok = 0,
    /// Driver tx path was busy
    Busy = 1,
}

/// Network device statistics (atomic for multicore safety)
#[repr(C)]
#[derive(Debug)]
pub struct NetDeviceStats {
    pub rx_packets: AtomicU64,
    pub tx_packets: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub tx_bytes: AtomicU64,
    pub rx_errors: AtomicU64,
    pub tx_errors: AtomicU64,
    pub rx_dropped: AtomicU64,
    pub tx_dropped: AtomicU64,
    pub multicast: AtomicU64,
    pub collisions: AtomicU64,
    pub rx_length_errors: AtomicU64,
    pub rx_over_errors: AtomicU64,
    pub rx_crc_errors: AtomicU64,
    pub rx_frame_errors: AtomicU64,
    pub rx_fifo_errors: AtomicU64,
    pub rx_missed_errors: AtomicU64,
    pub tx_aborted_errors: AtomicU64,
    pub tx_carrier_errors: AtomicU64,
    pub tx_fifo_errors: AtomicU64,
    pub tx_heartbeat_errors: AtomicU64,
    pub tx_window_errors: AtomicU64,
    pub rx_compressed: AtomicU64,
    pub tx_compressed: AtomicU64,
}

impl Default for NetDeviceStats {
    fn default() -> Self {
        Self {
            rx_packets: AtomicU64::new(0),
            tx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            rx_errors: AtomicU64::new(0),
            tx_errors: AtomicU64::new(0),
            rx_dropped: AtomicU64::new(0),
            tx_dropped: AtomicU64::new(0),
            multicast: AtomicU64::new(0),
            collisions: AtomicU64::new(0),
            rx_length_errors: AtomicU64::new(0),
            rx_over_errors: AtomicU64::new(0),
            rx_crc_errors: AtomicU64::new(0),
            rx_frame_errors: AtomicU64::new(0),
            rx_fifo_errors: AtomicU64::new(0),
            rx_missed_errors: AtomicU64::new(0),
            tx_aborted_errors: AtomicU64::new(0),
            tx_carrier_errors: AtomicU64::new(0),
            tx_fifo_errors: AtomicU64::new(0),
            tx_heartbeat_errors: AtomicU64::new(0),
            tx_window_errors: AtomicU64::new(0),
            rx_compressed: AtomicU64::new(0),
            tx_compressed: AtomicU64::new(0),
        }
    }
}

impl Clone for NetDeviceStats {
    fn clone(&self) -> Self {
        Self {
            rx_packets: AtomicU64::new(self.rx_packets.load(Ordering::Relaxed)),
            tx_packets: AtomicU64::new(self.tx_packets.load(Ordering::Relaxed)),
            rx_bytes: AtomicU64::new(self.rx_bytes.load(Ordering::Relaxed)),
            tx_bytes: AtomicU64::new(self.tx_bytes.load(Ordering::Relaxed)),
            rx_errors: AtomicU64::new(self.rx_errors.load(Ordering::Relaxed)),
            tx_errors: AtomicU64::new(self.tx_errors.load(Ordering::Relaxed)),
            rx_dropped: AtomicU64::new(self.rx_dropped.load(Ordering::Relaxed)),
            tx_dropped: AtomicU64::new(self.tx_dropped.load(Ordering::Relaxed)),
            multicast: AtomicU64::new(self.multicast.load(Ordering::Relaxed)),
            collisions: AtomicU64::new(self.collisions.load(Ordering::Relaxed)),
            rx_length_errors: AtomicU64::new(self.rx_length_errors.load(Ordering::Relaxed)),
            rx_over_errors: AtomicU64::new(self.rx_over_errors.load(Ordering::Relaxed)),
            rx_crc_errors: AtomicU64::new(self.rx_crc_errors.load(Ordering::Relaxed)),
            rx_frame_errors: AtomicU64::new(self.rx_frame_errors.load(Ordering::Relaxed)),
            rx_fifo_errors: AtomicU64::new(self.rx_fifo_errors.load(Ordering::Relaxed)),
            rx_missed_errors: AtomicU64::new(self.rx_missed_errors.load(Ordering::Relaxed)),
            tx_aborted_errors: AtomicU64::new(self.tx_aborted_errors.load(Ordering::Relaxed)),
            tx_carrier_errors: AtomicU64::new(self.tx_carrier_errors.load(Ordering::Relaxed)),
            tx_fifo_errors: AtomicU64::new(self.tx_fifo_errors.load(Ordering::Relaxed)),
            tx_heartbeat_errors: AtomicU64::new(self.tx_heartbeat_errors.load(Ordering::Relaxed)),
            tx_window_errors: AtomicU64::new(self.tx_window_errors.load(Ordering::Relaxed)),
            rx_compressed: AtomicU64::new(self.rx_compressed.load(Ordering::Relaxed)),
            tx_compressed: AtomicU64::new(self.tx_compressed.load(Ordering::Relaxed)),
        }
    }
}

/// Network device queue
#[repr(C)]
pub struct NetDevQueue {
    /// Queue state
    pub state: AtomicU32,
    /// Queue lock
    pub lock: Mutex<()>,
    /// Transmit queue
    pub qdisc: Option<NonNull<QDisc>>,
}

/// Placeholder for QDisc (traffic control)
#[repr(C)]
pub struct QDisc {
    _placeholder: u8,
}

/// NAPI (New API) structure for efficient packet processing
#[repr(C)]
pub struct Napi {
    /// Poll list
    pub poll_list: Option<NonNull<Napi>>,
    /// Poll function
    pub poll: Option<fn(&mut Napi, budget: i32) -> i32>,
    /// Weight (max packets per poll)
    pub weight: i32,
    /// Device this NAPI belongs to
    pub dev: Option<NonNull<NetDevice>>,
    /// NAPI state
    pub state: AtomicU32,
}

/// Network device structure
///
/// This is the core structure representing a network interface.
/// Simplified but production-ready version of Linux's net_device.
#[repr(C)]
pub struct NetDevice {
    // ========== Identification ==========
    /// Interface name (e.g., "eth0", "wlan0")
    pub name: [u8; IFNAMSIZ],
    /// Interface index
    pub ifindex: u32,
    /// Device type (Ethernet, WiFi, etc)
    pub dev_type: u16,
    
    // ========== State ==========
    /// Device flags
    pub flags: AtomicU32,
    /// Device state
    pub state: NetDevState,
    /// Operational state
    pub operstate: u8,
    /// Link mode
    pub link_mode: u8,
    
    // ========== Hardware info ==========
    /// Hardware address (MAC)
    pub dev_addr: [u8; MAX_ADDR_LEN],
    /// Permanent hardware address
    pub perm_addr: [u8; MAX_ADDR_LEN],
    /// Broadcast address
    pub broadcast: [u8; MAX_ADDR_LEN],
    /// Address length
    pub addr_len: u8,
    
    // ========== MTU and headers ==========
    /// Maximum transmission unit
    pub mtu: u32,
    /// Minimum MTU
    pub min_mtu: u32,
    /// Maximum MTU
    pub max_mtu: u32,
    /// Hard header length
    pub hard_header_len: u16,
    /// Needed headroom
    pub needed_headroom: u16,
    /// Needed tailroom
    pub needed_tailroom: u16,
    
    // ========== Features ==========
    /// Device features
    pub features: NetdevFeatures,
    /// Hardware features
    pub hw_features: NetdevFeatures,
    /// Wanted features
    pub wanted_features: NetdevFeatures,
    /// VLAN features
    pub vlan_features: NetdevFeatures,
    
    // ========== GSO/GRO ==========
    /// GSO maximum size
    pub gso_max_size: u32,
    /// GSO maximum segments
    pub gso_max_segs: u16,
    /// GRO maximum size
    pub gro_max_size: u32,
    /// TSO maximum size
    pub tso_max_size: u32,
    /// TSO maximum segments
    pub tso_max_segs: u16,
    
    // ========== Queues ==========
    /// Number of TX queues
    pub num_tx_queues: u32,
    /// Number of RX queues
    pub num_rx_queues: u32,
    /// Real number of TX queues
    pub real_num_tx_queues: u32,
    /// Real number of RX queues
    pub real_num_rx_queues: u32,
    /// TX queue length
    pub tx_queue_len: u32,
    
    // ========== Statistics ==========
    /// Device statistics (atomic, no lock needed)
    pub stats: NetDeviceStats,
    
    // ========== Operations ==========
    /// Device operations
    pub netdev_ops: &'static NetDeviceOps,
    
    // ========== Reference counting ==========
    /// Reference count
    pub refcnt: AtomicU32,
    
    // ========== NAPI ==========
    /// NAPI list
    pub napi_list: Mutex<Vec<Box<Napi>>>,
    
    // ========== Watchdog ==========
    /// Watchdog timeout (jiffies)
    pub watchdog_timeo: u32,
    
    // ========== Private data ==========
    /// Driver private data
    pub priv_data: Option<NonNull<u8>>,
    /// Private data size
    pub priv_len: u32,
    
    // ========== IRQ ==========
    /// IRQ number
    pub irq: i32,
    
    // ========== Promiscuity ==========
    /// Promiscuity count
    pub promiscuity: AtomicU32,
    /// All-multicast count
    pub allmulti: AtomicU32,
    
    // ========== Protocol down ==========
    /// Protocol down flag
    pub proto_down: AtomicBool,
    /// Protocol down reason
    pub proto_down_reason: u32,
}

// SAFETY: NetDevice contains NonNull pointers but all access is synchronized
// through DEVICE_REGISTRY Mutex. Pointers remain valid for device lifetime.
unsafe impl Send for NetDevice {}

impl NetDevice {
    /// Allocate a new network device
    ///
    /// Ported from: `alloc_netdev_mqs()`
    pub fn alloc(
        name: &str,
        dev_type: u16,
        netdev_ops: &'static NetDeviceOps,
        num_tx_queues: u32,
        num_rx_queues: u32,
    ) -> Result<Box<Self>, NetDevError> {
        if name.len() >= IFNAMSIZ {
            return Err(NetDevError::NameTooLong);
        }
        
        let mut name_buf = [0u8; IFNAMSIZ];
        name_buf[..name.len()].copy_from_slice(name.as_bytes());
        
        Ok(Box::new(Self {
            name: name_buf,
            ifindex: 0, // Assigned during registration
            dev_type,
            flags: AtomicU32::new(0),
            state: NetDevState::Init,
            operstate: 0,
            link_mode: 0,
            dev_addr: [0; MAX_ADDR_LEN],
            perm_addr: [0; MAX_ADDR_LEN],
            broadcast: [0xff; MAX_ADDR_LEN],
            addr_len: 6, // Ethernet default
            mtu: 1500, // Ethernet default
            min_mtu: 68,
            max_mtu: 65535,
            hard_header_len: 14, // Ethernet header
            needed_headroom: 0,
            needed_tailroom: 0,
            features: 0,
            hw_features: 0,
            wanted_features: 0,
            vlan_features: 0,
            gso_max_size: 65536,
            gso_max_segs: 65535,
            gro_max_size: 65536,
            tso_max_size: 65536,
            tso_max_segs: 65535,
            num_tx_queues,
            num_rx_queues,
            real_num_tx_queues: num_tx_queues,
            real_num_rx_queues: num_rx_queues,
            tx_queue_len: 1000,
            stats: NetDeviceStats::default(),
            netdev_ops,
            refcnt: AtomicU32::new(1),
            napi_list: Mutex::new(Vec::new()),
            watchdog_timeo: 5 * 100, // 5 seconds in jiffies
            priv_data: None,
            priv_len: 0,
            irq: -1,
            promiscuity: AtomicU32::new(0),
            allmulti: AtomicU32::new(0),
            proto_down: AtomicBool::new(false),
            proto_down_reason: 0,
        }))
    }
    
    /// Get interface name as string
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(IFNAMSIZ);
        core::str::from_utf8(&self.name[..end]).unwrap_or("<invalid>")
    }
    
    /// Open the device
    ///
    /// Ported from: `dev_open()`
    pub fn open(&mut self) -> Result<(), NetDevError> {
        // Check if already up
        if self.flags.load(Ordering::Relaxed) & net_device_flags::IFF_UP != 0 {
            return Ok(());
        }
        
        // Call driver's open
        if let Some(ndo_open) = self.netdev_ops.ndo_open {
            ndo_open(self)?;
        }
        
        // Set flags
        self.flags.fetch_or(
            net_device_flags::IFF_UP | net_device_flags::IFF_RUNNING,
            Ordering::Relaxed
        );
        
        Ok(())
    }
    
    /// Close the device
    ///
    /// Ported from: `dev_close()`
    pub fn close(&mut self) -> Result<(), NetDevError> {
        // Check if already down
        if self.flags.load(Ordering::Relaxed) & net_device_flags::IFF_UP == 0 {
            return Ok(());
        }
        
        // Clear flags
        self.flags.fetch_and(
            !(net_device_flags::IFF_UP | net_device_flags::IFF_RUNNING),
            Ordering::Relaxed
        );
        
        // Call driver's stop
        if let Some(ndo_stop) = self.netdev_ops.ndo_stop {
            ndo_stop(self)?;
        }
        
        Ok(())
    }
    
    /// Transmit a packet
    ///
    /// Ported from: `dev_queue_xmit()`
    pub fn xmit(&mut self, skb: Box<SkBuff>) -> Result<(), NetDevError> {
        // Check if device is up
        if self.flags.load(Ordering::Relaxed) & net_device_flags::IFF_UP == 0 {
            return Err(NetDevError::Down);
        }
        
        // Call driver's transmit
        if let Some(ndo_start_xmit) = self.netdev_ops.ndo_start_xmit {
            match ndo_start_xmit(self, skb) {
                NetDevTx::Ok => Ok(()),
                NetDevTx::Busy => Err(NetDevError::Busy),
            }
        } else {
            Err(NetDevError::NotSupported)
        }
    }
    
    /// Change MTU
    ///
    /// Ported from: `dev_set_mtu()`
    pub fn set_mtu(&mut self, new_mtu: u32) -> Result<(), NetDevError> {
        if new_mtu < self.min_mtu || new_mtu > self.max_mtu {
            return Err(NetDevError::InvalidMtu);
        }
        
        if let Some(ndo_change_mtu) = self.netdev_ops.ndo_change_mtu {
            ndo_change_mtu(self, new_mtu)?;
        }
        
        self.mtu = new_mtu;
        Ok(())
    }
    
    /// Set MAC address
    ///
    /// Ported from: `dev_set_mac_address()`
    pub fn set_mac_address(&mut self, addr: &[u8]) -> Result<(), NetDevError> {
        if addr.len() != self.addr_len as usize {
            return Err(NetDevError::InvalidAddress);
        }
        
        if let Some(ndo_set_mac) = self.netdev_ops.ndo_set_mac_address {
            ndo_set_mac(self, addr)?;
        }
        
        self.dev_addr[..addr.len()].copy_from_slice(addr);
        Ok(())
    }
    
    /// Get statistics
    pub fn get_stats(&self) -> NetDeviceStats {
        if let Some(ndo_get_stats64) = self.netdev_ops.ndo_get_stats64 {
            ndo_get_stats64(self)
        } else {
            self.stats.clone()
        }
    }
    
    /// Increment reference count
    ///
    /// Ported from: `dev_hold()`
    #[inline]
    pub fn hold(&self) {
        self.refcnt.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Decrement reference count
    ///
    /// Ported from: `dev_put()`
    #[inline]
    pub fn put(&self) {
        let old = self.refcnt.fetch_sub(1, Ordering::Release);
        if old == 1 {
            // Last reference dropped - device can be freed
            core::sync::atomic::fence(Ordering::Acquire);
        }
    }
}

/// Global device registry
static DEVICE_REGISTRY: Mutex<DeviceRegistry> = Mutex::new(DeviceRegistry::new());

struct DeviceRegistry {
    devices: Vec<Box<NetDevice>>,
    next_ifindex: u32,
}

impl DeviceRegistry {
    const fn new() -> Self {
        Self {
            devices: Vec::new(),
            next_ifindex: 1,
        }
    }
}

/// Device handle with automatic reference counting
pub struct DeviceHandle {
    ifindex: u32,
}

impl DeviceHandle {
    fn new(ifindex: u32) -> Self {
        // Increment refcount
        let registry = DEVICE_REGISTRY.lock();
        if let Some(dev) = registry.devices.iter().find(|d| d.ifindex == ifindex) {
            dev.hold();
        }
        Self { ifindex }
    }
    
    /// Get device interface index
    pub fn ifindex(&self) -> u32 {
        self.ifindex
    }
    
    /// Execute operation with device
    pub fn with_device<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&NetDevice) -> R,
    {
        let registry = DEVICE_REGISTRY.lock();
        registry.devices.iter()
            .find(|d| d.ifindex == self.ifindex)
            .map(|d| f(d.as_ref()))
    }
    
    /// Execute mutable operation with device
    pub fn with_device_mut<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut NetDevice) -> R,
    {
        let mut registry = DEVICE_REGISTRY.lock();
        registry.devices.iter_mut()
            .find(|d| d.ifindex == self.ifindex)
            .map(|d| f(d.as_mut()))
    }
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        // Decrement refcount
        let registry = DEVICE_REGISTRY.lock();
        if let Some(dev) = registry.devices.iter().find(|d| d.ifindex == self.ifindex) {
            dev.put();
        }
    }
}

impl Clone for DeviceHandle {
    fn clone(&self) -> Self {
        Self::new(self.ifindex)
    }
}

/// Register a network device
///
/// Ported from: `register_netdev()`
pub fn register_netdev(mut dev: Box<NetDevice>) -> Result<u32, NetDevError> {
    let mut registry = DEVICE_REGISTRY.lock();
    
    // Assign interface index
    dev.ifindex = registry.next_ifindex;
    registry.next_ifindex += 1;
    
    // Initialize device
    if let Some(ndo_init) = dev.netdev_ops.ndo_init {
        ndo_init(&mut dev)?;
    }
    
    // Update state
    dev.state = NetDevState::Registered;
    
    let ifindex = dev.ifindex;
    registry.devices.push(dev);
    
    Ok(ifindex)
}

/// Unregister a network device
///
/// Ported from: `unregister_netdev()`
pub fn unregister_netdev(ifindex: u32) -> Result<(), NetDevError> {
    let mut registry = DEVICE_REGISTRY.lock();
    
    let pos = registry.devices.iter().position(|d| d.ifindex == ifindex)
        .ok_or(NetDevError::NotFound)?;
    
    let mut dev = registry.devices.remove(pos);
    
    // Close device if open
    let _ = dev.close();
    
    // Uninitialize
    if let Some(ndo_uninit) = dev.netdev_ops.ndo_uninit {
        ndo_uninit(&mut dev);
    }
    
    dev.state = NetDevState::Unregistered;
    
    Ok(())
}

/// Find device by interface index
///
/// Returns a handle with automatic reference counting.
/// Device will not be freed while handle exists.
///
/// Ported from: `dev_get_by_index()`
pub fn dev_get_by_index(ifindex: u32) -> Option<DeviceHandle> {
    let registry = DEVICE_REGISTRY.lock();
    registry.devices.iter()
        .find(|d| d.ifindex == ifindex)
        .map(|_| DeviceHandle::new(ifindex))
}

/// Find device by name
///
/// Returns a handle with automatic reference counting.
///
/// Ported from: `dev_get_by_name()`
pub fn dev_get_by_name(name: &str) -> Option<DeviceHandle> {
    let registry = DEVICE_REGISTRY.lock();
    registry.devices.iter()
        .find(|d| d.name_str() == name)
        .map(|d| DeviceHandle::new(d.ifindex))
}

/// Network device errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetDevError {
    /// Device not found
    NotFound,
    /// Device is down
    Down,
    /// Device is busy
    Busy,
    /// Operation not supported
    NotSupported,
    /// Invalid MTU
    InvalidMtu,
    /// Invalid address
    InvalidAddress,
    /// Name too long
    NameTooLong,
    /// Allocation failed
    AllocFailed,
    /// Hardware error
    HwError,
}
