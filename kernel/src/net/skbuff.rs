// SPDX-License-Identifier: MIT OR Apache-2.0
//! Socket Buffer (sk_buff)
//!
//! Ported from Linux: `include/linux/skbuff.h`, `net/core/skbuff.c`
//!
//! The sk_buff is the fundamental data structure for network packets in Linux.
//! It contains metadata about the packet and pointers to the actual data.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;

/// Maximum control buffer size (48 bytes in Linux)
pub const SKB_CB_SIZE: usize = 48;

/// Checksum types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChecksumType {
    /// Device did not checksum this packet
    None = 0,
    /// Hardware verified checksum
    Unnecessary = 1,
    /// Hardware computed full checksum
    Complete = 2,
    /// Checksum offloaded to hardware
    Partial = 3,
}

/// Packet types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PacketType {
    /// Packet for this host
    Host = 0,
    /// Packet for another host (we're routing)
    Otherhost = 1,
    /// Broadcast packet
    Broadcast = 2,
    /// Multicast packet
    Multicast = 3,
    /// Packet with invalid dest address
    Loopback = 4,
    /// Packet originated from this host
    Outgoing = 5,
}

/// Clone status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CloneStatus {
    /// Not cloned
    Unavailable = 0,
    /// Original buffer
    Orig = 1,
    /// Clone of original
    Clone = 2,
}

/// Timestamp type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TimestampType {
    Realtime = 0,
    Monotonic = 1,
    Tai = 2,
}

/// GSO (Generic Segmentation Offload) types
pub mod gso_type {
    pub const TCPV4: u32 = 1 << 0;
    pub const DODGY: u32 = 1 << 1;
    pub const TCP_ECN: u32 = 1 << 2;
    pub const TCP_FIXEDID: u32 = 1 << 3;
    pub const TCPV6: u32 = 1 << 4;
    pub const FCOE: u32 = 1 << 5;
    pub const GRE: u32 = 1 << 6;
    pub const GRE_CSUM: u32 = 1 << 7;
    pub const IPXIP4: u32 = 1 << 8;
    pub const IPXIP6: u32 = 1 << 9;
    pub const UDP_TUNNEL: u32 = 1 << 10;
    pub const UDP_TUNNEL_CSUM: u32 = 1 << 11;
    pub const PARTIAL: u32 = 1 << 12;
    pub const TUNNEL_REMCSUM: u32 = 1 << 13;
    pub const SCTP: u32 = 1 << 14;
    pub const ESP: u32 = 1 << 15;
    pub const UDP: u32 = 1 << 16;
    pub const UDP_L4: u32 = 1 << 17;
    pub const FRAGLIST: u32 = 1 << 18;
}

/// Shared info for sk_buff fragments
#[repr(C)]
pub struct SkbSharedInfo {
    /// GSO type flags
    pub gso_type: u32,
    /// GSO segment size
    pub gso_size: u16,
    /// Number of GSO segments
    pub gso_segs: u16,
    /// Number of fragments
    pub nr_frags: u8,
    /// TX flags
    pub tx_flags: u8,
    /// Fragment list
    pub frag_list: Option<NonNull<SkBuff>>,
    /// Page fragments (up to 17 in Linux)
    pub frags: [PageFrag; MAX_SKB_FRAGS],
}

/// Maximum number of page fragments
pub const MAX_SKB_FRAGS: usize = 17;

/// Page fragment descriptor
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PageFrag {
    /// Physical page address
    pub page: u64,
    /// Offset within page
    pub offset: u32,
    /// Length of data
    pub len: u32,
}

impl Default for PageFrag {
    fn default() -> Self {
        Self { page: 0, offset: 0, len: 0 }
    }
}

/// Socket buffer - the core network packet structure
///
/// This is a simplified but production-ready version of Linux's sk_buff.
/// It maintains the same memory layout and semantics for compatibility.
#[repr(C)]
pub struct SkBuff {
    // ========== List linkage ==========
    /// Next buffer in list
    pub next: Option<NonNull<SkBuff>>,
    /// Previous buffer in list
    pub prev: Option<NonNull<SkBuff>>,
    
    // ========== Device and socket ==========
    /// Device we arrived on/are leaving by
    pub dev: Option<NonNull<NetDevice>>,
    /// Socket we are owned by
    pub sk: Option<NonNull<Socket>>,
    
    // ========== Timestamps ==========
    /// Time we arrived/left (nanoseconds)
    pub tstamp: u64,
    /// Timestamp type
    pub tstamp_type: TimestampType,
    
    // ========== Control buffer ==========
    /// Control buffer - free for use by each layer
    pub cb: [u8; SKB_CB_SIZE],
    
    // ========== Length fields ==========
    /// Total length of data
    pub len: u32,
    /// Length of paged data
    pub data_len: u32,
    /// Length of link layer header
    pub mac_len: u16,
    /// Writable header length (for clones)
    pub hdr_len: u16,
    
    // ========== Checksum fields ==========
    /// Checksum value
    pub csum: u32,
    /// Checksum type
    pub ip_summed: ChecksumType,
    /// Offset from head where checksumming starts
    pub csum_start: u16,
    /// Offset from csum_start where checksum is stored
    pub csum_offset: u16,
    /// Checksum level (nested checksums)
    pub csum_level: u8,
    /// Use CRC32c instead of Internet checksum
    pub csum_not_inet: bool,
    /// Checksum was completed by software
    pub csum_complete_sw: bool,
    /// Checksum is valid
    pub csum_valid: bool,
    
    // ========== Priority and QoS ==========
    /// Packet queueing priority
    pub priority: u32,
    /// Queue mapping for multiqueue devices
    pub queue_mapping: u16,
    /// Traffic control index
    pub tc_index: u16,
    /// Packet hash
    pub hash: u32,
    /// 4-tuple hash indicator
    pub l4_hash: bool,
    /// Software computed hash
    pub sw_hash: bool,
    
    // ========== Packet classification ==========
    /// Packet type
    pub pkt_type: PacketType,
    /// Protocol from driver
    pub protocol: u16,
    /// Inner protocol (for encapsulation)
    pub inner_protocol: u16,
    
    // ========== Flags ==========
    /// Clone status
    pub cloned: CloneStatus,
    /// Allow local fragmentation
    pub ignore_df: bool,
    /// Payload reference only
    pub nohdr: bool,
    /// Packet has been seen (for stats)
    pub peeked: bool,
    /// Allocated from page fragments
    pub head_frag: bool,
    /// Allocated from PFMEMALLOC reserves
    pub pfmemalloc: bool,
    /// Mark for page_pool recycling
    pub pp_recycle: bool,
    /// OK to change queue mapping
    pub ooo_okay: bool,
    /// Request NIC to omit FCS
    pub no_fcs: bool,
    /// Inner headers are valid
    pub encapsulation: bool,
    /// Software checksum needed for encap header
    pub encap_hdr_csum: bool,
    /// Remote checksum offload enabled
    pub remcsum_offload: bool,
    /// Packet was L2-forwarded in hardware
    pub offload_fwd_mark: bool,
    /// Packet was L3-forwarded in hardware
    pub offload_l3_fwd_mark: bool,
    /// Skip TC classification
    pub tc_skip_classify: bool,
    /// Within TC classify (ingress)
    pub tc_at_ingress: bool,
    /// Packet was redirected
    pub redirected: bool,
    /// Redirected from ingress
    pub from_ingress: bool,
    /// Skip netfilter egress
    pub nf_skip_egress: bool,
    /// Netfilter trace flag
    pub nf_trace: bool,
    /// IPvs property
    pub ipvs_property: bool,
    /// WiFi ACK valid
    pub wifi_acked_valid: bool,
    /// WiFi ACK received
    pub wifi_acked: bool,
    /// Destination pending confirm
    pub dst_pending_confirm: bool,
    /// Packet was decrypted
    pub decrypted: bool,
    /// Slow GRO path needed
    pub slow_gro: bool,
    /// Fragment is unreadable
    pub unreadable: bool,
    
    // ========== VLAN fields ==========
    /// VLAN protocol
    pub vlan_proto: u16,
    /// VLAN TCI (tag control information)
    pub vlan_tci: u16,
    
    // ========== Header pointers ==========
    /// Transport layer header offset
    pub transport_header: u16,
    /// Network layer header offset
    pub network_header: u16,
    /// Link layer header offset
    pub mac_header: u16,
    /// Inner transport header offset (encapsulation)
    pub inner_transport_header: u16,
    /// Inner network header offset (encapsulation)
    pub inner_network_header: u16,
    /// Inner MAC header offset (encapsulation)
    pub inner_mac_header: u16,
    
    // ========== Data pointers ==========
    /// Head of buffer
    pub head: NonNull<u8>,
    /// Data head pointer (offset from head)
    pub data: u16,
    /// Tail pointer (offset from head)
    pub tail: u16,
    /// End pointer (offset from head)
    pub end: u16,
    
    // ========== Memory management ==========
    /// Buffer size (including shared info)
    pub truesize: u32,
    /// Reference count
    pub users: AtomicU32,
    /// Destructor function
    pub destructor: Option<fn(&mut SkBuff)>,
    
    // ========== Interface index ==========
    /// Interface index we arrived on
    pub skb_iif: u32,
    
    // ========== Security ==========
    /// Security marking
    pub secmark: u32,
    /// Generic packet mark
    pub mark: u32,
    
    // ========== NAPI ==========
    /// NAPI ID
    pub napi_id: u32,
    /// CPU that allocated this skb
    pub alloc_cpu: u16,
}

/// Placeholder for network device
#[repr(C)]
pub struct NetDevice {
    _placeholder: u8,
}

/// Placeholder for socket
#[repr(C)]
pub struct Socket {
    _placeholder: u8,
}

impl SkBuff {
    /// Allocate a new sk_buff with specified size
    ///
    /// Ported from: `__alloc_skb()`
    ///
    /// # Security
    /// - Validates size against MAX_SKB_SIZE
    /// - Validates size fits in u16 for internal offset tracking
    pub fn alloc(size: usize, _gfp_mask: u32) -> Result<Box<Self>, SkBuffError> {
        if size > MAX_SKB_SIZE {
            return Err(SkBuffError::TooLarge);
        }
        
        // Internal offsets (data, tail, end) are u16, so size must fit
        if size > u16::MAX as usize {
            return Err(SkBuffError::TooLarge);
        }
        
        // Allocate data buffer
        let mut data_buf = Vec::with_capacity(size + SKB_SHARED_INFO_SIZE);
        data_buf.resize(size + SKB_SHARED_INFO_SIZE, 0);
        
        let head = NonNull::new(data_buf.as_mut_ptr())
            .ok_or(SkBuffError::AllocFailed)?;
        
        let skb = Box::new(Self {
            next: None,
            prev: None,
            dev: None,
            sk: None,
            tstamp: 0,
            tstamp_type: TimestampType::Monotonic,
            cb: [0; SKB_CB_SIZE],
            len: 0,
            data_len: 0,
            mac_len: 0,
            hdr_len: 0,
            csum: 0,
            ip_summed: ChecksumType::None,
            csum_start: 0,
            csum_offset: 0,
            csum_level: 0,
            csum_not_inet: false,
            csum_complete_sw: false,
            csum_valid: false,
            priority: 0,
            queue_mapping: 0,
            tc_index: 0,
            hash: 0,
            l4_hash: false,
            sw_hash: false,
            pkt_type: PacketType::Host,
            protocol: 0,
            inner_protocol: 0,
            cloned: CloneStatus::Unavailable,
            ignore_df: false,
            nohdr: false,
            peeked: false,
            head_frag: false,
            pfmemalloc: false,
            pp_recycle: false,
            ooo_okay: false,
            no_fcs: false,
            encapsulation: false,
            encap_hdr_csum: false,
            remcsum_offload: false,
            offload_fwd_mark: false,
            offload_l3_fwd_mark: false,
            tc_skip_classify: false,
            tc_at_ingress: false,
            redirected: false,
            from_ingress: false,
            nf_skip_egress: false,
            nf_trace: false,
            ipvs_property: false,
            wifi_acked_valid: false,
            wifi_acked: false,
            dst_pending_confirm: false,
            decrypted: false,
            slow_gro: false,
            unreadable: false,
            vlan_proto: 0,
            vlan_tci: 0,
            transport_header: 0,
            network_header: 0,
            mac_header: 0,
            inner_transport_header: 0,
            inner_network_header: 0,
            inner_mac_header: 0,
            head,
            data: 0,
            tail: 0,
            end: size as u16,
            truesize: (size + SKB_SHARED_INFO_SIZE + core::mem::size_of::<Self>()) as u32,
            users: AtomicU32::new(1),
            destructor: None,
            skb_iif: 0,
            secmark: 0,
            mark: 0,
            napi_id: 0,
            alloc_cpu: 0,
        });
        
        // Leak the data buffer - it's now owned by skb
        core::mem::forget(data_buf);
        
        Ok(skb)
    }
    
    /// Reserve headroom in the buffer
    ///
    /// Ported from: `skb_reserve()`
    ///
    /// # Panics
    /// Debug-asserts that reservation doesn't exceed buffer end.
    #[inline]
    pub fn reserve(&mut self, len: usize) {
        debug_assert!(self.data as usize + len <= self.end as usize,
            "skb_reserve: headroom exceeds buffer");
        self.data += len as u16;
        self.tail += len as u16;
    }
    
    /// Add data to the end of the buffer
    ///
    /// Ported from: `skb_put()`
    pub fn put(&mut self, len: usize) -> Result<&mut [u8], SkBuffError> {
        let new_tail = self.tail + len as u16;
        if new_tail > self.end {
            return Err(SkBuffError::Overflow);
        }
        
        let old_tail = self.tail;
        self.tail = new_tail;
        self.len += len as u32;
        
        unsafe {
            let ptr = self.head.as_ptr().add(old_tail as usize);
            Ok(core::slice::from_raw_parts_mut(ptr, len))
        }
    }
    
    /// Remove data from the start of the buffer
    ///
    /// Ported from: `skb_pull()`
    pub fn pull(&mut self, len: usize) -> Result<(), SkBuffError> {
        if len > self.len as usize {
            return Err(SkBuffError::Underflow);
        }
        
        self.data += len as u16;
        self.len -= len as u32;
        Ok(())
    }
    
    /// Add data to the start of the buffer
    ///
    /// Ported from: `skb_push()`
    pub fn push(&mut self, len: usize) -> Result<&mut [u8], SkBuffError> {
        if len > self.data as usize {
            return Err(SkBuffError::Underflow);
        }
        
        self.data -= len as u16;
        self.len += len as u32;
        
        unsafe {
            let ptr = self.head.as_ptr().add(self.data as usize);
            Ok(core::slice::from_raw_parts_mut(ptr, len))
        }
    }
    
    /// Get pointer to data
    #[inline]
    pub fn data_ptr(&self) -> *const u8 {
        unsafe { self.head.as_ptr().add(self.data as usize) }
    }
    
    /// Get mutable pointer to data
    #[inline]
    pub fn data_ptr_mut(&mut self) -> *mut u8 {
        unsafe { self.head.as_ptr().add(self.data as usize) }
    }
    
    /// Get data as slice
    #[inline]
    pub fn data_slice(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.data_ptr(), self.len as usize)
        }
    }
    
    /// Get data as mutable slice
    #[inline]
    pub fn data_slice_mut(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.data_ptr_mut(), self.len as usize)
        }
    }
    
    /// Get headroom size
    #[inline]
    pub fn headroom(&self) -> usize {
        self.data as usize
    }
    
    /// Get tailroom size
    #[inline]
    pub fn tailroom(&self) -> usize {
        (self.end - self.tail) as usize
    }
    
    /// Clone the sk_buff (shallow copy)
    ///
    /// Ported from: `skb_clone()`
    pub fn clone(&self) -> Result<Box<Self>, SkBuffError> {
        // Increment reference count on original
        self.users.fetch_add(1, Ordering::Relaxed);
        
        let cloned = Box::new(Self {
            next: None,
            prev: None,
            dev: self.dev,
            sk: None, // Socket is not cloned
            tstamp: self.tstamp,
            tstamp_type: self.tstamp_type,
            cb: self.cb,
            len: self.len,
            data_len: self.data_len,
            mac_len: self.mac_len,
            hdr_len: self.hdr_len,
            csum: self.csum,
            ip_summed: self.ip_summed,
            csum_start: self.csum_start,
            csum_offset: self.csum_offset,
            csum_level: self.csum_level,
            csum_not_inet: self.csum_not_inet,
            csum_complete_sw: self.csum_complete_sw,
            csum_valid: self.csum_valid,
            priority: self.priority,
            queue_mapping: self.queue_mapping,
            tc_index: self.tc_index,
            hash: self.hash,
            l4_hash: self.l4_hash,
            sw_hash: self.sw_hash,
            pkt_type: self.pkt_type,
            protocol: self.protocol,
            inner_protocol: self.inner_protocol,
            cloned: CloneStatus::Clone,
            ignore_df: self.ignore_df,
            nohdr: true, // Clones can't modify header
            peeked: self.peeked,
            head_frag: self.head_frag,
            pfmemalloc: self.pfmemalloc,
            pp_recycle: false, // Don't recycle clones
            ooo_okay: self.ooo_okay,
            no_fcs: self.no_fcs,
            encapsulation: self.encapsulation,
            encap_hdr_csum: self.encap_hdr_csum,
            remcsum_offload: self.remcsum_offload,
            offload_fwd_mark: self.offload_fwd_mark,
            offload_l3_fwd_mark: self.offload_l3_fwd_mark,
            tc_skip_classify: self.tc_skip_classify,
            tc_at_ingress: self.tc_at_ingress,
            redirected: self.redirected,
            from_ingress: self.from_ingress,
            nf_skip_egress: self.nf_skip_egress,
            nf_trace: self.nf_trace,
            ipvs_property: self.ipvs_property,
            wifi_acked_valid: self.wifi_acked_valid,
            wifi_acked: self.wifi_acked,
            dst_pending_confirm: self.dst_pending_confirm,
            decrypted: self.decrypted,
            slow_gro: self.slow_gro,
            unreadable: self.unreadable,
            vlan_proto: self.vlan_proto,
            vlan_tci: self.vlan_tci,
            transport_header: self.transport_header,
            network_header: self.network_header,
            mac_header: self.mac_header,
            inner_transport_header: self.inner_transport_header,
            inner_network_header: self.inner_network_header,
            inner_mac_header: self.inner_mac_header,
            head: self.head,
            data: self.data,
            tail: self.tail,
            end: self.end,
            truesize: self.truesize,
            users: AtomicU32::new(1),
            destructor: None,
            skb_iif: self.skb_iif,
            secmark: self.secmark,
            mark: self.mark,
            napi_id: self.napi_id,
            alloc_cpu: self.alloc_cpu,
        });
        
        Ok(cloned)
    }
}

impl Drop for SkBuff {
    fn drop(&mut self) {
        // Call destructor if set
        if let Some(destructor) = self.destructor {
            destructor(self);
        }
        
        // Decrement reference count
        let old_users = self.users.fetch_sub(1, Ordering::Release);
        
        // If this was the last reference, free the data buffer
        if old_users == 1 {
            // Acquire fence ensures all writes from other cores are visible
            // before we free the memory
            core::sync::atomic::fence(Ordering::Acquire);
            
            unsafe {
                let size = self.end as usize + SKB_SHARED_INFO_SIZE;
                let _ = Vec::from_raw_parts(self.head.as_ptr(), size, size);
            }
        }
    }
}

/// Maximum sk_buff size (64KB)
pub const MAX_SKB_SIZE: usize = 65536;

/// Size of skb_shared_info structure
pub const SKB_SHARED_INFO_SIZE: usize = 320;

/// sk_buff errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkBuffError {
    /// Allocation failed
    AllocFailed,
    /// Buffer too large
    TooLarge,
    /// Buffer overflow
    Overflow,
    /// Buffer underflow
    Underflow,
    /// Invalid operation
    Invalid,
}
