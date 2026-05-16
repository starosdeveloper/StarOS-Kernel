//! VLESS Protocol - Kernel-level tunnel for censorship circumvention
//!
//! Implements the VLESS protocol (V2Ray/Xray) at the kernel network layer,
//! providing native transparent proxying without userspace overhead.
//!
//! Architecture:
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │  Applications (any app, no proxy config needed)  │
//! ├──────────────────────────────────────────────────┤
//! │  VLESS Tunnel Interface (tun0)                   │
//! │  - Intercepts outbound traffic                   │
//! │  - Encapsulates in VLESS frames                  │
//! │  - Routes through configured server              │
//! ├──────────────────────────────────────────────────┤
//! │  Transport Layer (WebSocket / gRPC / Reality)    │
//! ├──────────────────────────────────────────────────┤
//! │  TLS 1.3 (with Reality fingerprint)              │
//! ├──────────────────────────────────────────────────┤
//! │  TCP/UDP → Remote VLESS Server                   │
//! └──────────────────────────────────────────────────┘
//! ```

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use crate::error::KernelError;

// ---------------------------------------------------------------------------
// VLESS Protocol Constants
// ---------------------------------------------------------------------------

/// VLESS protocol version
pub const VLESS_VERSION: u8 = 0;

/// Maximum payload per VLESS frame (16KB)
pub const VLESS_MAX_PAYLOAD: usize = 16384;

/// UUID length (16 bytes)
pub const UUID_LEN: usize = 16;

/// VLESS command types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VlessCommand {
    /// TCP connection
    Tcp = 0x01,
    /// UDP connection
    Udp = 0x02,
    /// Mux (multiplexed connections)
    Mux = 0x03,
}

/// VLESS address type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AddressType {
    IPv4 = 0x01,
    Domain = 0x02,
    IPv6 = 0x03,
}

// ---------------------------------------------------------------------------
// VLESS Connection Configuration
// ---------------------------------------------------------------------------

/// UUID for authentication (16 bytes)
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Uuid([u8; UUID_LEN]);

impl Uuid {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Parse UUID from string "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
    pub fn parse(s: &str) -> Result<Self, KernelError> {
        let bytes = s.as_bytes();
        if bytes.len() != 36 {
            return Err(KernelError::InvalidParameter("UUID must be 36 chars"));
        }
        let mut uuid = [0u8; 16];
        let mut idx = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'-' { i += 1; continue; }
            if i + 1 >= bytes.len() { return Err(KernelError::InvalidParameter("invalid UUID")); }
            uuid[idx] = hex_byte(bytes[i], bytes[i + 1])?;
            idx += 1;
            i += 2;
        }
        if idx != 16 { return Err(KernelError::InvalidParameter("invalid UUID length")); }
        Ok(Self(uuid))
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// VLESS server configuration
#[derive(Clone)]
pub struct VlessConfig {
    /// Server address (IP or domain)
    pub server_addr: [u8; 256],
    pub server_addr_len: usize,
    /// Server port
    pub server_port: u16,
    /// Authentication UUID
    pub uuid: Uuid,
    /// Transport type
    pub transport: TransportType,
    /// Flow control (xtls-rprx-vision)
    pub flow: FlowType,
    /// SNI for TLS
    pub sni: [u8; 256],
    pub sni_len: usize,
    /// Enable/disable
    pub enabled: bool,
}

/// Transport protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    /// Raw TCP
    Tcp,
    /// WebSocket
    WebSocket,
    /// gRPC
    Grpc,
    /// Reality (anti-detection TLS)
    Reality,
}

/// Flow control type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowType {
    None,
    /// XTLS Vision - splice read/write for zero-copy
    XtlsVision,
}

// ---------------------------------------------------------------------------
// VLESS Frame Format
// ---------------------------------------------------------------------------

/// VLESS request header
/// Format: [version(1)] [uuid(16)] [addon_len(1)] [addon(...)] [cmd(1)] [port(2)] [addr_type(1)] [addr(...)]
pub struct VlessRequest {
    pub uuid: Uuid,
    pub command: VlessCommand,
    pub dest_port: u16,
    pub dest_addr: DestAddress,
}

/// Destination address
#[derive(Clone)]
pub enum DestAddress {
    IPv4([u8; 4]),
    IPv6([u8; 16]),
    Domain(Vec<u8>),
}

impl VlessRequest {
    /// Serialize VLESS request header
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(64);

        // Version
        buf.push(VLESS_VERSION);

        // UUID (16 bytes)
        buf.extend_from_slice(self.uuid.as_bytes());

        // Addon length (0 for no addons)
        buf.push(0);

        // Command
        buf.push(self.command as u8);

        // Destination port (big-endian)
        buf.extend_from_slice(&self.dest_port.to_be_bytes());

        // Address type + address
        match &self.dest_addr {
            DestAddress::IPv4(addr) => {
                buf.push(AddressType::IPv4 as u8);
                buf.extend_from_slice(addr);
            }
            DestAddress::IPv6(addr) => {
                buf.push(AddressType::IPv6 as u8);
                buf.extend_from_slice(addr);
            }
            DestAddress::Domain(domain) => {
                buf.push(AddressType::Domain as u8);
                buf.push(domain.len() as u8);
                buf.extend_from_slice(domain);
            }
        }

        buf
    }

    /// Parse VLESS response header (just version + addon_len)
    pub fn decode_response(data: &[u8]) -> Result<usize, KernelError> {
        if data.len() < 2 {
            return Err(KernelError::InvalidParameter("VLESS response too short"));
        }
        if data[0] != VLESS_VERSION {
            return Err(KernelError::InvalidParameter("VLESS version mismatch"));
        }
        let addon_len = data[1] as usize;
        Ok(2 + addon_len) // header size consumed
    }
}

// ---------------------------------------------------------------------------
// VLESS Tunnel State
// ---------------------------------------------------------------------------

/// Connection state for a single VLESS tunnel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelState {
    Disconnected,
    Connecting,
    Handshake,
    Active,
    Closing,
}

/// Statistics for the tunnel
pub struct TunnelStats {
    pub bytes_tx: AtomicU64,
    pub bytes_rx: AtomicU64,
    pub packets_tx: AtomicU64,
    pub packets_rx: AtomicU64,
    pub connections: AtomicU64,
}

impl TunnelStats {
    pub const fn new() -> Self {
        Self {
            bytes_tx: AtomicU64::new(0),
            bytes_rx: AtomicU64::new(0),
            packets_tx: AtomicU64::new(0),
            packets_rx: AtomicU64::new(0),
            connections: AtomicU64::new(0),
        }
    }
}

/// The kernel VLESS tunnel manager
pub struct VlessTunnel {
    config: Mutex<Option<VlessConfig>>,
    state: Mutex<TunnelState>,
    stats: TunnelStats,
    active: AtomicBool,
}

impl VlessTunnel {
    pub const fn new() -> Self {
        Self {
            config: Mutex::new(None),
            state: Mutex::new(TunnelState::Disconnected),
            stats: TunnelStats::new(),
            active: AtomicBool::new(false),
        }
    }

    /// Configure the tunnel with server parameters
    pub fn configure(&self, config: VlessConfig) -> Result<(), KernelError> {
        if config.server_addr_len == 0 || config.server_port == 0 {
            return Err(KernelError::InvalidParameter("invalid server config"));
        }
        *self.config.lock() = Some(config);
        Ok(())
    }

    /// Start the tunnel
    pub fn start(&self) -> Result<(), KernelError> {
        let config = self.config.lock();
        if config.is_none() {
            return Err(KernelError::NotInitialized);
        }
        if !config.as_ref().unwrap().enabled {
            return Err(KernelError::InvalidParameter("tunnel disabled"));
        }
        drop(config);

        *self.state.lock() = TunnelState::Connecting;
        self.active.store(true, Ordering::Release);
        self.stats.connections.fetch_add(1, Ordering::Relaxed);

        // In production: establish TCP connection to server,
        // perform TLS handshake, send VLESS auth
        *self.state.lock() = TunnelState::Active;
        Ok(())
    }

    /// Stop the tunnel
    pub fn stop(&self) {
        self.active.store(false, Ordering::Release);
        *self.state.lock() = TunnelState::Disconnected;
    }

    /// Encapsulate outbound packet in VLESS frame
    pub fn encapsulate(&self, dest_ip: &[u8], dest_port: u16, payload: &[u8]) -> Result<Vec<u8>, KernelError> {
        if !self.active.load(Ordering::Acquire) {
            return Err(KernelError::NotInitialized);
        }
        if payload.len() > VLESS_MAX_PAYLOAD {
            return Err(KernelError::InvalidParameter("payload too large"));
        }

        let dest_addr = match dest_ip.len() {
            4 => DestAddress::IPv4(dest_ip.try_into().unwrap()),
            16 => DestAddress::IPv6(dest_ip.try_into().unwrap()),
            _ => return Err(KernelError::InvalidAddress),
        };

        let config = self.config.lock();
        let cfg = config.as_ref().ok_or(KernelError::NotInitialized)?;

        let request = VlessRequest {
            uuid: cfg.uuid,
            command: VlessCommand::Tcp,
            dest_port,
            dest_addr,
        };

        let mut frame = request.encode();
        frame.extend_from_slice(payload);

        self.stats.bytes_tx.fetch_add(payload.len() as u64, Ordering::Relaxed);
        self.stats.packets_tx.fetch_add(1, Ordering::Relaxed);

        Ok(frame)
    }

    /// Decapsulate inbound VLESS frame, return payload
    pub fn decapsulate(&self, data: &[u8]) -> Result<Vec<u8>, KernelError> {
        if !self.active.load(Ordering::Acquire) {
            return Err(KernelError::NotInitialized);
        }

        let header_len = VlessRequest::decode_response(data)?;
        if data.len() <= header_len {
            return Ok(Vec::new());
        }

        let payload = data[header_len..].to_vec();
        self.stats.bytes_rx.fetch_add(payload.len() as u64, Ordering::Relaxed);
        self.stats.packets_rx.fetch_add(1, Ordering::Relaxed);

        Ok(payload)
    }

    /// Get tunnel state
    pub fn state(&self) -> TunnelState {
        *self.state.lock()
    }

    /// Check if tunnel is active
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Get statistics
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (
            self.stats.bytes_tx.load(Ordering::Relaxed),
            self.stats.bytes_rx.load(Ordering::Relaxed),
            self.stats.packets_tx.load(Ordering::Relaxed),
            self.stats.packets_rx.load(Ordering::Relaxed),
        )
    }
}

/// Global tunnel instance
static VLESS_TUNNEL: VlessTunnel = VlessTunnel::new();

/// Get the global VLESS tunnel
pub fn tunnel() -> &'static VlessTunnel {
    &VLESS_TUNNEL
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hex_byte(hi: u8, lo: u8) -> Result<u8, KernelError> {
    Ok((hex_nibble(hi)? << 4) | hex_nibble(lo)?)
}

fn hex_nibble(c: u8) -> Result<u8, KernelError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(KernelError::InvalidParameter("invalid hex")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_parse() {
        let uuid = Uuid::parse("12345678-1234-1234-1234-123456789abc").unwrap();
        assert_eq!(uuid.0[0], 0x12);
        assert_eq!(uuid.0[15], 0xbc);
    }

    #[test]
    fn test_vless_request_encode() {
        let req = VlessRequest {
            uuid: Uuid::from_bytes([1; 16]),
            command: VlessCommand::Tcp,
            dest_port: 443,
            dest_addr: DestAddress::IPv4([1, 1, 1, 1]),
        };
        let encoded = req.encode();
        assert_eq!(encoded[0], VLESS_VERSION);
        assert_eq!(encoded[1..17], [1; 16]); // UUID
        assert_eq!(encoded[17], 0); // addon len
        assert_eq!(encoded[18], 0x01); // TCP command
        assert_eq!(encoded[19..21], 443u16.to_be_bytes()); // port
        assert_eq!(encoded[21], 0x01); // IPv4
        assert_eq!(encoded[22..26], [1, 1, 1, 1]); // addr
    }

    #[test]
    fn test_tunnel_lifecycle() {
        let tunnel = VlessTunnel::new();
        assert_eq!(tunnel.state(), TunnelState::Disconnected);

        // Can't start without config
        assert!(tunnel.start().is_err());

        let mut config = VlessConfig {
            server_addr: [0; 256],
            server_addr_len: 11,
            server_port: 443,
            uuid: Uuid::from_bytes([0xAB; 16]),
            transport: TransportType::Reality,
            flow: FlowType::XtlsVision,
            sni: [0; 256],
            sni_len: 0,
            enabled: true,
        };
        config.server_addr[..11].copy_from_slice(b"example.com");

        tunnel.configure(config).unwrap();
        tunnel.start().unwrap();
        assert_eq!(tunnel.state(), TunnelState::Active);
        assert!(tunnel.is_active());

        // Encapsulate
        let frame = tunnel.encapsulate(&[8, 8, 8, 8], 53, b"hello").unwrap();
        assert!(frame.len() > 5);

        tunnel.stop();
        assert_eq!(tunnel.state(), TunnelState::Disconnected);
    }

    #[test]
    fn test_decapsulate() {
        let tunnel = VlessTunnel::new();
        let mut config = VlessConfig {
            server_addr: [0; 256],
            server_addr_len: 5,
            server_port: 443,
            uuid: Uuid::from_bytes([1; 16]),
            transport: TransportType::Tcp,
            flow: FlowType::None,
            sni: [0; 256],
            sni_len: 0,
            enabled: true,
        };
        config.server_addr[..5].copy_from_slice(b"1.2.3");
        tunnel.configure(config).unwrap();
        tunnel.start().unwrap();

        // Simulate response: version(0) + addon_len(0) + payload
        let response = [0u8, 0, 0xDE, 0xAD, 0xBE, 0xEF];
        let payload = tunnel.decapsulate(&response).unwrap();
        assert_eq!(payload, &[0xDE, 0xAD, 0xBE, 0xEF]);
    }
}
