// SPDX-License-Identifier: MIT OR Apache-2.0
//! Network Stack
//!
//! Ported from Linux: `net/core/`, `net/ipv4/`
//!
//! Phase 12: Core networking + TCP/IP basics
//!
//! Architecture:
//! ```text
//! ┌─────────────────────────────────────────┐
//! │  Application Layer                      │
//! ├─────────────────────────────────────────┤
//! │  TCP/UDP (transport layer)              │
//! ├─────────────────────────────────────────┤
//! │  IPv4/IPv6 (network layer)              │
//! ├─────────────────────────────────────────┤
//! │  Device Layer (link layer)              │
//! ├─────────────────────────────────────────┤
//! │  sk_buff (packet buffer management)     │
//! └─────────────────────────────────────────┘
//! ```

#[cfg(not(test))]
pub mod skbuff;
#[cfg(not(test))]
pub mod dev;
#[cfg(not(test))]
pub mod ipv4;
#[cfg(not(test))]
pub mod ipv6;
pub mod mac80211;
pub mod bluetooth;

#[cfg(not(test))]
pub use skbuff::{SkBuff, SkBuffError, ChecksumType, PacketType};
#[cfg(not(test))]
pub use dev::{
    NetDevice, NetDeviceOps, NetDeviceStats, NetDevError, NetDevTx, DeviceHandle,
    register_netdev, unregister_netdev, dev_get_by_index, dev_get_by_name,
    net_device_flags, netdev_features,
};
#[cfg(not(test))]
pub use ipv4::{IpHeader, IpError, ip_rcv, ip_output, ping, ipv4_addr};
#[cfg(not(test))]
pub use ipv6::{Ipv6Addr, Ipv6Header, Ipv6Error, ipv6_rcv, ipv6_output, ping6, ipv6_addr_const};
