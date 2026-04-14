// SPDX-License-Identifier: MIT OR Apache-2.0
//! IPv4 Protocol Implementation
//!
//! Ported from Linux: `net/ipv4/ip_input.c`, `net/ipv4/ip_output.c`
//!
//! Handles IPv4 packet processing, routing, and transmission.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, Ordering};
use crate::net::skbuff::SkBuff;
use crate::net::dev::{NetDevice, DeviceHandle};

/// IPv4 version
pub const IPVERSION: u8 = 4;

/// Default TTL
pub const IPDEFTTL: u8 = 64;

/// Maximum TTL
pub const MAXTTL: u8 = 255;

/// Minimum IPv4 header length (20 bytes)
pub const MIN_IP_HEADER_LEN: usize = 20;

/// Maximum IPv4 header length (60 bytes with options)
pub const MAX_IP_HEADER_LEN: usize = 60;

/// IPv4 protocols
pub mod ip_protocol {
    pub const IPPROTO_IP: u8 = 0;       // Dummy protocol
    pub const IPPROTO_ICMP: u8 = 1;     // Internet Control Message Protocol
    pub const IPPROTO_IGMP: u8 = 2;     // Internet Group Management Protocol
    pub const IPPROTO_IPIP: u8 = 4;     // IPIP tunnels
    pub const IPPROTO_TCP: u8 = 6;      // Transmission Control Protocol
    pub const IPPROTO_EGP: u8 = 8;      // Exterior Gateway Protocol
    pub const IPPROTO_PUP: u8 = 12;     // PUP protocol
    pub const IPPROTO_UDP: u8 = 17;     // User Datagram Protocol
    pub const IPPROTO_IDP: u8 = 22;     // XNS IDP protocol
    pub const IPPROTO_TP: u8 = 29;      // SO Transport Protocol Class 4
    pub const IPPROTO_DCCP: u8 = 33;    // Datagram Congestion Control Protocol
    pub const IPPROTO_IPV6: u8 = 41;    // IPv6-in-IPv4 tunnelling
    pub const IPPROTO_RSVP: u8 = 46;    // RSVP Protocol
    pub const IPPROTO_GRE: u8 = 47;     // Cisco GRE tunnels
    pub const IPPROTO_ESP: u8 = 50;     // Encapsulation Security Payload
    pub const IPPROTO_AH: u8 = 51;      // Authentication Header
    pub const IPPROTO_MTP: u8 = 92;     // Multicast Transport Protocol
    pub const IPPROTO_BEETPH: u8 = 94;  // IP option pseudo header for BEET
    pub const IPPROTO_ENCAP: u8 = 98;   // Encapsulation Header
    pub const IPPROTO_PIM: u8 = 103;    // Protocol Independent Multicast
    pub const IPPROTO_COMP: u8 = 108;   // Compression Header Protocol
    pub const IPPROTO_SCTP: u8 = 132;   // Stream Control Transport Protocol
    pub const IPPROTO_UDPLITE: u8 = 136; // UDP-Lite
    pub const IPPROTO_MPLS: u8 = 137;   // MPLS in IP
    pub const IPPROTO_RAW: u8 = 255;    // Raw IP packets
}

/// Type of Service (TOS) values
pub mod ip_tos {
    pub const IPTOS_TOS_MASK: u8 = 0x1E;
    pub const IPTOS_LOWDELAY: u8 = 0x10;
    pub const IPTOS_THROUGHPUT: u8 = 0x08;
    pub const IPTOS_RELIABILITY: u8 = 0x04;
    pub const IPTOS_MINCOST: u8 = 0x02;
    
    pub const IPTOS_PREC_MASK: u8 = 0xE0;
    pub const IPTOS_PREC_NETCONTROL: u8 = 0xe0;
    pub const IPTOS_PREC_INTERNETCONTROL: u8 = 0xc0;
    pub const IPTOS_PREC_CRITIC_ECP: u8 = 0xa0;
    pub const IPTOS_PREC_FLASHOVERRIDE: u8 = 0x80;
    pub const IPTOS_PREC_FLASH: u8 = 0x60;
    pub const IPTOS_PREC_IMMEDIATE: u8 = 0x40;
    pub const IPTOS_PREC_PRIORITY: u8 = 0x20;
    pub const IPTOS_PREC_ROUTINE: u8 = 0x00;
}

/// Fragment flags
pub mod ip_frag {
    pub const IP_CE: u16 = 0x8000;      // Congestion Experienced
    pub const IP_DF: u16 = 0x4000;      // Don't Fragment
    pub const IP_MF: u16 = 0x2000;      // More Fragments
    pub const IP_OFFSET: u16 = 0x1FFF;  // Fragment offset mask
}

/// IPv4 header
///
/// Ported from: `struct iphdr`
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct IpHeader {
    /// Version (4 bits) and IHL (4 bits)
    pub version_ihl: u8,
    /// Type of Service
    pub tos: u8,
    /// Total length
    pub tot_len: u16,
    /// Identification
    pub id: u16,
    /// Fragment offset and flags
    pub frag_off: u16,
    /// Time to Live
    pub ttl: u8,
    /// Protocol
    pub protocol: u8,
    /// Header checksum
    pub check: u16,
    /// Source address
    pub saddr: u32,
    /// Destination address
    pub daddr: u32,
}

impl IpHeader {
    /// Create new IPv4 header
    pub fn new(saddr: u32, daddr: u32, protocol: u8, tot_len: u16) -> Self {
        Self {
            version_ihl: (IPVERSION << 4) | 5, // Version 4, IHL 5 (20 bytes)
            tos: 0,
            tot_len: tot_len.to_be(),
            id: 0, // Will be set by ip_select_ident
            frag_off: 0,
            ttl: IPDEFTTL,
            protocol,
            check: 0, // Will be calculated
            saddr: saddr.to_be(),
            daddr: daddr.to_be(),
        }
    }
    
    /// Get IP version
    #[inline]
    pub fn version(&self) -> u8 {
        self.version_ihl >> 4
    }
    
    /// Get header length in bytes
    #[inline]
    pub fn ihl(&self) -> usize {
        ((self.version_ihl & 0x0F) as usize) * 4
    }
    
    /// Get total length
    #[inline]
    pub fn tot_len(&self) -> u16 {
        u16::from_be(self.tot_len)
    }
    
    /// Get identification
    #[inline]
    pub fn id(&self) -> u16 {
        u16::from_be(self.id)
    }
    
    /// Get fragment offset
    #[inline]
    pub fn frag_off(&self) -> u16 {
        u16::from_be(self.frag_off)
    }
    
    /// Check if Don't Fragment flag is set
    #[inline]
    pub fn is_df(&self) -> bool {
        self.frag_off() & ip_frag::IP_DF != 0
    }
    
    /// Check if More Fragments flag is set
    #[inline]
    pub fn is_mf(&self) -> bool {
        self.frag_off() & ip_frag::IP_MF != 0
    }
    
    /// Get fragment offset in bytes
    #[inline]
    pub fn fragment_offset(&self) -> u16 {
        (self.frag_off() & ip_frag::IP_OFFSET) * 8
    }
    
    /// Get source address
    #[inline]
    pub fn saddr(&self) -> u32 {
        u32::from_be(self.saddr)
    }
    
    /// Get destination address
    #[inline]
    pub fn daddr(&self) -> u32 {
        u32::from_be(self.daddr)
    }
    
    /// Calculate and set checksum
    ///
    /// Ported from: `ip_fast_csum()`
    pub fn calculate_checksum(&mut self) {
        self.check = 0;
        
        let words = unsafe {
            core::slice::from_raw_parts(
                self as *const _ as *const u16,
                self.ihl() / 2
            )
        };
        
        let mut sum: u32 = 0;
        for &word in words {
            sum += u16::from_be(word) as u32;
        }
        
        // Fold 32-bit sum to 16 bits
        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        
        self.check = (!sum as u16).to_be();
    }
    
    /// Verify checksum
    pub fn verify_checksum(&self) -> bool {
        let words = unsafe {
            core::slice::from_raw_parts(
                self as *const _ as *const u16,
                self.ihl() / 2
            )
        };
        
        let mut sum: u32 = 0;
        for &word in words {
            sum += u16::from_be(word) as u32;
        }
        
        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        
        sum == 0xFFFF
    }
}

/// Global IP identification counter
static IP_IDENT: AtomicU16 = AtomicU16::new(0);

/// Select IP identification
///
/// Ported from: `ip_select_ident()`
pub fn ip_select_ident() -> u16 {
    IP_IDENT.fetch_add(1, Ordering::Relaxed)
}

/// IPv4 input processing
///
/// Ported from: `ip_rcv()`
pub fn ip_rcv(skb: Box<SkBuff>, dev: &DeviceHandle) -> Result<(), IpError> {
    // Get IP header
    let iph = unsafe {
        &*(skb.data_ptr() as *const IpHeader)
    };
    
    // Sanity checks
    if skb.len < MIN_IP_HEADER_LEN as u32 {
        return Err(IpError::TooShort);
    }
    
    if iph.version() != IPVERSION {
        return Err(IpError::BadVersion);
    }
    
    let ihl = iph.ihl();
    if ihl < MIN_IP_HEADER_LEN {
        return Err(IpError::BadHeaderLen);
    }
    
    if skb.len < ihl as u32 {
        return Err(IpError::TooShort);
    }
    
    // Verify checksum
    if !iph.verify_checksum() {
        return Err(IpError::BadChecksum);
    }
    
    let tot_len = iph.tot_len() as u32;
    if skb.len < tot_len || tot_len < ihl as u32 {
        return Err(IpError::BadLength);
    }
    
    // Trim padding
    if skb.len > tot_len {
        // Would call skb_trim here
    }
    
    // Check TTL
    if iph.ttl == 0 {
        return Err(IpError::TtlExpired);
    }
    
    // Deliver to upper layer
    ip_local_deliver(skb)
}

/// Deliver packet to local protocol handler
///
/// Ported from: `ip_local_deliver()`
fn ip_local_deliver(skb: Box<SkBuff>) -> Result<(), IpError> {
    let iph = unsafe {
        &*(skb.data_ptr() as *const IpHeader)
    };
    
    // Handle fragmentation if needed
    if iph.is_mf() || iph.fragment_offset() != 0 {
        return ip_defrag(skb);
    }
    
    ip_local_deliver_finish(skb)
}

/// Final delivery to protocol handler
///
/// Ported from: `ip_local_deliver_finish()`
fn ip_local_deliver_finish(mut skb: Box<SkBuff>) -> Result<(), IpError> {
    let iph = unsafe {
        &*(skb.data_ptr() as *const IpHeader)
    };
    
    // Pull IP header
    let ihl = iph.ihl();
    let _ = skb.pull(ihl);
    
    // Deliver to protocol handler
    match iph.protocol {
        ip_protocol::IPPROTO_ICMP => {
            // ICMP handler
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
            Err(IpError::UnknownProtocol)
        }
    }
}

/// Handle IP fragmentation
///
/// Ported from: `ip_defrag()`
fn ip_defrag(skb: Box<SkBuff>) -> Result<(), IpError> {
    // Simplified: drop fragments for now
    // Full implementation would reassemble fragments
    Err(IpError::FragmentationNotSupported)
}

/// IPv4 output processing
///
/// Ported from: `ip_output()`
pub fn ip_output(
    mut skb: Box<SkBuff>,
    saddr: u32,
    daddr: u32,
    protocol: u8,
    dev: &mut DeviceHandle,
) -> Result<(), IpError> {
    let payload_len = skb.len as u16;
    let tot_len = MIN_IP_HEADER_LEN as u16 + payload_len;
    
    // Reserve space for IP header
    let iph_slice = skb.push(MIN_IP_HEADER_LEN)
        .map_err(|_| IpError::NoSpace)?;
    
    // Build IP header
    let mut iph = IpHeader::new(saddr, daddr, protocol, tot_len);
    iph.id = ip_select_ident().to_be();
    iph.calculate_checksum();
    
    // Copy header to skb
    unsafe {
        core::ptr::copy_nonoverlapping(
            &iph as *const _ as *const u8,
            iph_slice.as_mut_ptr(),
            MIN_IP_HEADER_LEN
        );
    }
    
    // Transmit
    dev.with_device_mut(|device| {
        device.xmit(skb)
    })
    .ok_or(IpError::DeviceNotFound)?
    .map_err(|_| IpError::TransmitFailed)
}

/// Send ICMP echo request (ping)
///
/// Ported from: `ping_v4_sendmsg()`
pub fn ping(
    daddr: u32,
    saddr: u32,
    dev: &mut DeviceHandle,
) -> Result<(), IpError> {
    // Allocate skb for ICMP echo request
    let mut skb = SkBuff::alloc(64, 0)
        .map_err(|_| IpError::AllocFailed)?;
    
    // Reserve space for headers
    skb.reserve(MIN_IP_HEADER_LEN + 8); // IP + ICMP header
    
    // Add ICMP payload (simplified)
    let payload = skb.put(32).map_err(|_| IpError::NoSpace)?;
    for (i, byte) in payload.iter_mut().enumerate() {
        *byte = i as u8;
    }
    
    // Send via IP layer
    ip_output(skb, saddr, daddr, ip_protocol::IPPROTO_ICMP, dev)
}

/// IPv4 errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpError {
    /// Packet too short
    TooShort,
    /// Bad IP version
    BadVersion,
    /// Bad header length
    BadHeaderLen,
    /// Bad checksum
    BadChecksum,
    /// Bad total length
    BadLength,
    /// TTL expired
    TtlExpired,
    /// Unknown protocol
    UnknownProtocol,
    /// Fragmentation not supported
    FragmentationNotSupported,
    /// No space in buffer
    NoSpace,
    /// Allocation failed
    AllocFailed,
    /// Device not found
    DeviceNotFound,
    /// Transmit failed
    TransmitFailed,
}

/// IPv4 address utilities
pub mod ipv4_addr {
    use alloc::vec::Vec;
    
    /// Convert IPv4 address from string (e.g., "192.168.1.1")
    pub fn from_str(s: &str) -> Option<u32> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 4 {
            return None;
        }
        
        let mut addr: u32 = 0;
        for (i, part) in parts.iter().enumerate() {
            let octet: u8 = part.parse().ok()?;
            addr |= (octet as u32) << (24 - i * 8);
        }
        
        Some(addr)
    }
    
    /// Convert IPv4 address to string
    pub fn to_string(addr: u32) -> alloc::string::String {
        alloc::format!(
            "{}.{}.{}.{}",
            (addr >> 24) & 0xFF,
            (addr >> 16) & 0xFF,
            (addr >> 8) & 0xFF,
            addr & 0xFF
        )
    }
    
    /// Check if address is loopback (127.0.0.0/8)
    pub fn is_loopback(addr: u32) -> bool {
        (addr >> 24) == 127
    }
    
    /// Check if address is private
    pub fn is_private(addr: u32) -> bool {
        let a = (addr >> 24) & 0xFF;
        let b = (addr >> 16) & 0xFF;
        
        // 10.0.0.0/8
        if a == 10 {
            return true;
        }
        
        // 172.16.0.0/12
        if a == 172 && (b >= 16 && b <= 31) {
            return true;
        }
        
        // 192.168.0.0/16
        if a == 192 && b == 168 {
            return true;
        }
        
        false
    }
    
    /// Check if address is multicast (224.0.0.0/4)
    pub fn is_multicast(addr: u32) -> bool {
        (addr >> 28) == 0xE
    }
    
    /// Check if address is broadcast
    pub fn is_broadcast(addr: u32) -> bool {
        addr == 0xFFFFFFFF
    }
}
