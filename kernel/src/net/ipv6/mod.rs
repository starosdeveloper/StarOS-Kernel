// SPDX-License-Identifier: MIT OR Apache-2.0
//! IPv6 Protocol Implementation
//!
//! Ported from Linux: `net/ipv6/ip6_input.c`, `net/ipv6/ip6_output.c`
//!
//! Handles IPv6 packet processing, routing, and transmission.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::net::skbuff::SkBuff;
use crate::net::dev::{NetDevice, DeviceHandle};
use crate::net::ipv4::ip_protocol;

/// IPv6 version
pub const IPV6VERSION: u8 = 6;

/// Default hop limit
pub const IPV6_DEFAULT_HOPLIMIT: u8 = 64;

/// Maximum hop limit
pub const IPV6_MAXHOPLIMIT: u8 = 255;

/// Minimum IPv6 MTU
pub const IPV6_MIN_MTU: u32 = 1280;

/// IPv6 header length (40 bytes, fixed)
pub const IPV6_HEADER_LEN: usize = 40;

/// IPv6 address length (128 bits = 16 bytes)
pub const IPV6_ADDR_LEN: usize = 16;

/// IPv6 address
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv6Addr {
    pub addr: [u8; IPV6_ADDR_LEN],
}

impl Ipv6Addr {
    /// Create new IPv6 address from bytes
    pub const fn new(addr: [u8; IPV6_ADDR_LEN]) -> Self {
        Self { addr }
    }
    
    /// Unspecified address (::)
    pub const fn unspecified() -> Self {
        Self::new([0; IPV6_ADDR_LEN])
    }
    
    /// Loopback address (::1)
    pub const fn loopback() -> Self {
        Self::new([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1])
    }
    
    /// Check if address is unspecified
    pub fn is_unspecified(&self) -> bool {
        self.addr.iter().all(|&b| b == 0)
    }
    
    /// Check if address is loopback
    pub fn is_loopback(&self) -> bool {
        self.addr[..15].iter().all(|&b| b == 0) && self.addr[15] == 1
    }
    
    /// Check if address is multicast (ff00::/8)
    pub fn is_multicast(&self) -> bool {
        self.addr[0] == 0xff
    }
    
    /// Check if address is link-local (fe80::/10)
    pub fn is_link_local(&self) -> bool {
        self.addr[0] == 0xfe && (self.addr[1] & 0xc0) == 0x80
    }
    
    /// Check if address is unique local (fc00::/7)
    pub fn is_unique_local(&self) -> bool {
        (self.addr[0] & 0xfe) == 0xfc
    }
    
    /// Check if address is global unicast
    pub fn is_global(&self) -> bool {
        !self.is_unspecified()
            && !self.is_loopback()
            && !self.is_multicast()
            && !self.is_link_local()
            && !self.is_unique_local()
    }
    
    /// Parse from string (e.g., "2001:db8::1")
    pub fn from_str(s: &str) -> Option<Self> {
        use alloc::vec::Vec;
        let mut addr = [0u8; IPV6_ADDR_LEN];
        let parts: Vec<&str> = s.split("::").collect();
        
        if parts.len() > 2 {
            return None;
        }
        
        let (left, right) = if parts.len() == 2 {
            (parts[0], parts[1])
        } else {
            (parts[0], "")
        };
        
        let mut pos = 0;
        
        // Parse left side
        if !left.is_empty() {
            for part in left.split(':') {
                if pos >= IPV6_ADDR_LEN {
                    return None;
                }
                let val = u16::from_str_radix(part, 16).ok()?;
                addr[pos] = (val >> 8) as u8;
                addr[pos + 1] = val as u8;
                pos += 2;
            }
        }
        
        // Parse right side (from end)
        if !right.is_empty() {
            let right_parts: Vec<&str> = right.split(':').collect();
            let mut right_pos = IPV6_ADDR_LEN - right_parts.len() * 2;
            
            for part in right_parts {
                if right_pos >= IPV6_ADDR_LEN {
                    return None;
                }
                let val = u16::from_str_radix(part, 16).ok()?;
                addr[right_pos] = (val >> 8) as u8;
                addr[right_pos + 1] = val as u8;
                right_pos += 2;
            }
        }
        
        Some(Self::new(addr))
    }
    
    /// Convert to string
    pub fn to_string(&self) -> alloc::string::String {
        use alloc::format;
        
        // Find longest sequence of zeros for compression
        let mut best_start = 0;
        let mut best_len = 0;
        let mut cur_start = 0;
        let mut cur_len = 0;
        
        for i in 0..8 {
            let word = u16::from_be_bytes([self.addr[i * 2], self.addr[i * 2 + 1]]);
            if word == 0 {
                if cur_len == 0 {
                    cur_start = i;
                }
                cur_len += 1;
            } else {
                if cur_len > best_len {
                    best_start = cur_start;
                    best_len = cur_len;
                }
                cur_len = 0;
            }
        }
        
        if cur_len > best_len {
            best_start = cur_start;
            best_len = cur_len;
        }
        
        let mut result = alloc::string::String::new();
        let mut i = 0;
        
        while i < 8 {
            if i == best_start && best_len > 1 {
                result.push_str("::");
                i += best_len;
            } else {
                if i > 0 && !(i == best_start + best_len && best_len > 1) {
                    result.push(':');
                }
                let word = u16::from_be_bytes([self.addr[i * 2], self.addr[i * 2 + 1]]);
                result.push_str(&format!("{:x}", word));
                i += 1;
            }
        }
        
        result
    }
}

/// IPv6 header
///
/// Ported from: `struct ipv6hdr`
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Ipv6Header {
    /// Version (4 bits), Traffic Class (8 bits), Flow Label (20 bits)
    pub version_tc_flow: u32,
    /// Payload length
    pub payload_len: u16,
    /// Next header (protocol)
    pub nexthdr: u8,
    /// Hop limit (TTL equivalent)
    pub hop_limit: u8,
    /// Source address
    pub saddr: Ipv6Addr,
    /// Destination address
    pub daddr: Ipv6Addr,
}

impl Ipv6Header {
    /// Create new IPv6 header
    pub fn new(saddr: Ipv6Addr, daddr: Ipv6Addr, nexthdr: u8, payload_len: u16) -> Self {
        Self {
            version_tc_flow: (IPV6VERSION as u32) << 28,
            payload_len: payload_len.to_be(),
            nexthdr,
            hop_limit: IPV6_DEFAULT_HOPLIMIT,
            saddr,
            daddr,
        }
    }
    
    /// Get IP version
    #[inline]
    pub fn version(&self) -> u8 {
        (u32::from_be(self.version_tc_flow) >> 28) as u8
    }
    
    /// Get traffic class
    #[inline]
    pub fn traffic_class(&self) -> u8 {
        ((u32::from_be(self.version_tc_flow) >> 20) & 0xFF) as u8
    }
    
    /// Get flow label
    #[inline]
    pub fn flow_label(&self) -> u32 {
        u32::from_be(self.version_tc_flow) & 0xFFFFF
    }
    
    /// Get payload length
    #[inline]
    pub fn payload_len(&self) -> u16 {
        u16::from_be(self.payload_len)
    }
    
    /// Set traffic class
    pub fn set_traffic_class(&mut self, tc: u8) {
        let mut val = u32::from_be(self.version_tc_flow);
        val = (val & !0x0FF00000) | ((tc as u32) << 20);
        self.version_tc_flow = val.to_be();
    }
    
    /// Set flow label
    pub fn set_flow_label(&mut self, flow: u32) {
        let mut val = u32::from_be(self.version_tc_flow);
        val = (val & !0xFFFFF) | (flow & 0xFFFFF);
        self.version_tc_flow = val.to_be();
    }
}

/// Global IPv6 flow label counter
static IPV6_FLOW_LABEL: AtomicU32 = AtomicU32::new(0);

/// Generate flow label
pub fn ipv6_flow_label() -> u32 {
    IPV6_FLOW_LABEL.fetch_add(1, Ordering::Relaxed) & 0xFFFFF
}

/// IPv6 input processing
///
/// Ported from: `ipv6_rcv()`
pub fn ipv6_rcv(skb: Box<SkBuff>, dev: &DeviceHandle) -> Result<(), Ipv6Error> {
    // Get IPv6 header
    let ip6h = unsafe {
        &*(skb.data_ptr() as *const Ipv6Header)
    };
    
    // Sanity checks
    if skb.len < IPV6_HEADER_LEN as u32 {
        return Err(Ipv6Error::TooShort);
    }
    
    if ip6h.version() != IPV6VERSION {
        return Err(Ipv6Error::BadVersion);
    }
    
    let payload_len = ip6h.payload_len() as u32;
    if skb.len < IPV6_HEADER_LEN as u32 + payload_len {
        return Err(Ipv6Error::TooShort);
    }
    
    // Check hop limit
    if ip6h.hop_limit == 0 {
        return Err(Ipv6Error::HopLimitExpired);
    }
    
    // Deliver to upper layer
    ipv6_local_deliver(skb)
}

/// Deliver packet to local protocol handler
///
/// Ported from: `ip6_input()`
fn ipv6_local_deliver(skb: Box<SkBuff>) -> Result<(), Ipv6Error> {
    let ip6h = unsafe {
        &*(skb.data_ptr() as *const Ipv6Header)
    };
    
    // Handle extension headers if needed
    let nexthdr = ip6h.nexthdr;
    
    ipv6_local_deliver_finish(skb, nexthdr)
}

/// Final delivery to protocol handler
///
/// Ported from: `ip6_input_finish()`
fn ipv6_local_deliver_finish(mut skb: Box<SkBuff>, nexthdr: u8) -> Result<(), Ipv6Error> {
    // Pull IPv6 header
    let _ = skb.pull(IPV6_HEADER_LEN);
    
    // Deliver to protocol handler
    match nexthdr {
        ip_protocol::IPPROTO_ICMP => {
            // ICMPv6 handler
            Ok(())
        }
        ip_protocol::IPPROTO_TCP => {
            // TCP handler
            Ok(())
        }
        ip_protocol::IPPROTO_UDP => {
            // UDP handler
            Ok(())
        }
        _ => {
            // Unknown protocol
            Err(Ipv6Error::UnknownProtocol)
        }
    }
}

/// IPv6 output processing
///
/// Ported from: `ip6_output()`
pub fn ipv6_output(
    mut skb: Box<SkBuff>,
    saddr: Ipv6Addr,
    daddr: Ipv6Addr,
    nexthdr: u8,
    dev: &mut DeviceHandle,
) -> Result<(), Ipv6Error> {
    let payload_len = skb.len as u16;
    
    // Reserve space for IPv6 header
    let ip6h_slice = skb.push(IPV6_HEADER_LEN)
        .map_err(|_| Ipv6Error::NoSpace)?;
    
    // Build IPv6 header
    let mut ip6h = Ipv6Header::new(saddr, daddr, nexthdr, payload_len);
    ip6h.set_flow_label(ipv6_flow_label());
    
    // Copy header to skb
    unsafe {
        core::ptr::copy_nonoverlapping(
            &ip6h as *const _ as *const u8,
            ip6h_slice.as_mut_ptr(),
            IPV6_HEADER_LEN
        );
    }
    
    // Transmit
    dev.with_device_mut(|device| {
        device.xmit(skb)
    })
    .ok_or(Ipv6Error::DeviceNotFound)?
    .map_err(|_| Ipv6Error::TransmitFailed)
}

/// Send ICMPv6 echo request (ping6)
///
/// Ported from: `ping_v6_sendmsg()`
pub fn ping6(
    daddr: Ipv6Addr,
    saddr: Ipv6Addr,
    dev: &mut DeviceHandle,
) -> Result<(), Ipv6Error> {
    // Allocate skb for ICMPv6 echo request
    let mut skb = SkBuff::alloc(64, 0)
        .map_err(|_| Ipv6Error::AllocFailed)?;
    
    // Reserve space for headers
    skb.reserve(IPV6_HEADER_LEN + 8); // IPv6 + ICMPv6 header
    
    // Add ICMPv6 payload (simplified)
    let payload = skb.put(32).map_err(|_| Ipv6Error::NoSpace)?;
    for (i, byte) in payload.iter_mut().enumerate() {
        *byte = i as u8;
    }
    
    // Send via IPv6 layer
    ipv6_output(
        skb,
        saddr,
        daddr,
        ip_protocol::IPPROTO_ICMP,
        dev
    )
}

/// IPv6 errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ipv6Error {
    /// Packet too short
    TooShort,
    /// Bad IP version
    BadVersion,
    /// Hop limit expired
    HopLimitExpired,
    /// Unknown protocol
    UnknownProtocol,
    /// No space in buffer
    NoSpace,
    /// Allocation failed
    AllocFailed,
    /// Device not found
    DeviceNotFound,
    /// Transmit failed
    TransmitFailed,
}

/// Well-known IPv6 addresses
pub mod ipv6_addr_const {
    use super::Ipv6Addr;
    
    /// Unspecified address (::)
    pub const UNSPECIFIED: Ipv6Addr = Ipv6Addr::unspecified();
    
    /// Loopback address (::1)
    pub const LOOPBACK: Ipv6Addr = Ipv6Addr::loopback();
    
    /// All nodes multicast (ff02::1)
    pub const ALL_NODES: Ipv6Addr = Ipv6Addr::new([
        0xff, 0x02, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0x01
    ]);
    
    /// All routers multicast (ff02::2)
    pub const ALL_ROUTERS: Ipv6Addr = Ipv6Addr::new([
        0xff, 0x02, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0x02
    ]);
}
