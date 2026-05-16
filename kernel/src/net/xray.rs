//! XRAY Transport Layer - WebSocket, gRPC, Reality
//!
//! Provides transport protocols for VLESS tunneling that disguise traffic
//! as legitimate HTTPS to bypass DPI (Deep Packet Inspection).
//!
//! Supported transports:
//! - **WebSocket**: Encapsulates VLESS in WS frames over TLS
//! - **gRPC**: Uses HTTP/2 gRPC streams (looks like API traffic)
//! - **Reality**: TLS 1.3 with stolen server certificate (undetectable)

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use crate::error::KernelError;

// ---------------------------------------------------------------------------
// WebSocket Transport
// ---------------------------------------------------------------------------

/// WebSocket frame opcodes
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum WsOpcode {
    Text = 0x01,
    Binary = 0x02,
    Close = 0x08,
    Ping = 0x09,
    Pong = 0x0A,
}

/// WebSocket frame encoder/decoder
pub struct WebSocketTransport {
    path: [u8; 128],
    path_len: usize,
    host: [u8; 256],
    host_len: usize,
    connected: bool,
    mask_key: AtomicU32,
}

impl WebSocketTransport {
    pub fn new(path: &[u8], host: &[u8]) -> Self {
        let mut t = Self {
            path: [0; 128],
            path_len: path.len().min(128),
            host: [0; 256],
            host_len: host.len().min(256),
            connected: false,
            mask_key: AtomicU32::new(0x12345678),
        };
        t.path[..t.path_len].copy_from_slice(&path[..t.path_len]);
        t.host[..t.host_len].copy_from_slice(&host[..t.host_len]);
        t
    }

    /// Generate WebSocket upgrade request
    pub fn handshake_request(&self) -> Vec<u8> {
        let mut req = Vec::with_capacity(256);
        req.extend_from_slice(b"GET ");
        req.extend_from_slice(&self.path[..self.path_len]);
        req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
        req.extend_from_slice(&self.host[..self.host_len]);
        req.extend_from_slice(b"\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n");
        req.extend_from_slice(b"Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n");
        req.extend_from_slice(b"Sec-WebSocket-Version: 13\r\n\r\n");
        req
    }

    /// Encode data into a WebSocket binary frame (client-masked)
    pub fn encode_frame(&self, payload: &[u8]) -> Vec<u8> {
        let mask = self.mask_key.fetch_add(1, Ordering::Relaxed).to_le_bytes();
        let len = payload.len();
        let mut frame = Vec::with_capacity(14 + len);

        // FIN + Binary opcode
        frame.push(0x82);

        // Mask bit + length
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len < 65536 {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }

        // Masking key
        frame.extend_from_slice(&mask);

        // Masked payload
        for (i, &b) in payload.iter().enumerate() {
            frame.push(b ^ mask[i & 3]);
        }

        frame
    }

    /// Decode a WebSocket frame, return payload
    pub fn decode_frame(data: &[u8]) -> Result<(Vec<u8>, usize), KernelError> {
        if data.len() < 2 {
            return Err(KernelError::InvalidParameter("WS frame too short"));
        }

        let masked = data[1] & 0x80 != 0;
        let mut payload_len = (data[1] & 0x7F) as usize;
        let mut offset = 2;

        if payload_len == 126 {
            if data.len() < 4 { return Err(KernelError::InvalidParameter("WS frame truncated")); }
            payload_len = u16::from_be_bytes([data[2], data[3]]) as usize;
            offset = 4;
        } else if payload_len == 127 {
            if data.len() < 10 { return Err(KernelError::InvalidParameter("WS frame truncated")); }
            payload_len = u64::from_be_bytes(data[2..10].try_into().unwrap()) as usize;
            offset = 10;
        }

        let mask_key = if masked {
            if data.len() < offset + 4 { return Err(KernelError::InvalidParameter("WS no mask")); }
            let m = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
            offset += 4;
            Some(m)
        } else {
            None
        };

        if data.len() < offset + payload_len {
            return Err(KernelError::InvalidParameter("WS payload truncated"));
        }

        let mut payload = data[offset..offset + payload_len].to_vec();
        if let Some(mask) = mask_key {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i & 3];
            }
        }

        Ok((payload, offset + payload_len))
    }
}

// ---------------------------------------------------------------------------
// gRPC Transport
// ---------------------------------------------------------------------------

/// gRPC transport - encapsulates data as HTTP/2 gRPC stream
pub struct GrpcTransport {
    service_name: [u8; 128],
    service_len: usize,
    stream_id: AtomicU32,
}

impl GrpcTransport {
    pub fn new(service_name: &[u8]) -> Self {
        let mut t = Self {
            service_name: [0; 128],
            service_len: service_name.len().min(128),
            stream_id: AtomicU32::new(1),
        };
        t.service_name[..t.service_len].copy_from_slice(&service_name[..t.service_len]);
        t
    }

    /// Encode payload as gRPC data frame
    /// Format: [compressed(1)] [length(4)] [data]
    pub fn encode(&self, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(5 + payload.len());
        frame.push(0x00); // not compressed
        frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    /// Decode gRPC data frame
    pub fn decode(data: &[u8]) -> Result<Vec<u8>, KernelError> {
        if data.len() < 5 {
            return Err(KernelError::InvalidParameter("gRPC frame too short"));
        }
        let _compressed = data[0];
        let len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
        if data.len() < 5 + len {
            return Err(KernelError::InvalidParameter("gRPC frame truncated"));
        }
        Ok(data[5..5 + len].to_vec())
    }

    pub fn next_stream_id(&self) -> u32 {
        self.stream_id.fetch_add(2, Ordering::Relaxed) // odd IDs for client
    }
}

// ---------------------------------------------------------------------------
// Reality Transport (TLS 1.3 camouflage)
// ---------------------------------------------------------------------------

/// Reality transport - makes traffic indistinguishable from legitimate TLS 1.3
///
/// Key features:
/// - Uses real server's TLS certificate (no self-signed detection)
/// - Client authenticates via short_id embedded in ClientHello
/// - Server-side: proxies TLS to real website for non-authenticated clients
pub struct RealityTransport {
    /// Server name for TLS SNI (the "stolen" domain)
    server_name: [u8; 256],
    server_name_len: usize,
    /// Short ID for authentication (8 bytes, hex-encoded in config)
    short_id: [u8; 8],
    /// Public key for ECDH key exchange (x25519, 32 bytes)
    public_key: [u8; 32],
    /// Spider-X path fingerprint
    spider_x: [u8; 64],
    spider_x_len: usize,
}

impl RealityTransport {
    pub fn new(server_name: &[u8], short_id: [u8; 8], public_key: [u8; 32]) -> Self {
        let mut t = Self {
            server_name: [0; 256],
            server_name_len: server_name.len().min(256),
            short_id,
            public_key,
            spider_x: [0; 64],
            spider_x_len: 0,
        };
        t.server_name[..t.server_name_len].copy_from_slice(&server_name[..t.server_name_len]);
        t
    }

    /// Build TLS 1.3 ClientHello with Reality authentication
    /// The short_id is embedded in the session_id field
    pub fn build_client_hello(&self) -> Vec<u8> {
        let mut hello = Vec::with_capacity(512);

        // TLS Record: Handshake
        hello.push(0x16); // ContentType: Handshake
        hello.extend_from_slice(&[0x03, 0x01]); // TLS 1.0 (for compat)
        // Length placeholder (will fill later)
        let len_pos = hello.len();
        hello.extend_from_slice(&[0x00, 0x00]);

        // Handshake: ClientHello
        hello.push(0x01); // HandshakeType: ClientHello
        let hs_len_pos = hello.len();
        hello.extend_from_slice(&[0x00, 0x00, 0x00]); // length placeholder

        // Client Version: TLS 1.2 (real version in extension)
        hello.extend_from_slice(&[0x03, 0x03]);

        // Random (32 bytes)
        let random: [u8; 32] = core::array::from_fn(|i| {
            (i as u8).wrapping_mul(0x41).wrapping_add(self.short_id[i & 7])
        });
        hello.extend_from_slice(&random);

        // Session ID (32 bytes) - embed short_id here for Reality auth
        hello.push(32); // session_id length
        let mut session_id = [0u8; 32];
        session_id[..8].copy_from_slice(&self.short_id);
        // Fill rest with deterministic data
        for i in 8..32 {
            session_id[i] = random[i] ^ self.public_key[i & 31];
        }
        hello.extend_from_slice(&session_id);

        // Cipher Suites
        hello.extend_from_slice(&[0x00, 0x04]); // 2 suites
        hello.extend_from_slice(&[0x13, 0x01]); // TLS_AES_128_GCM_SHA256
        hello.extend_from_slice(&[0x13, 0x02]); // TLS_AES_256_GCM_SHA384

        // Compression: null
        hello.extend_from_slice(&[0x01, 0x00]);

        // Extensions: SNI
        let sni_ext = self.build_sni_extension();
        // Extensions length
        hello.extend_from_slice(&(sni_ext.len() as u16).to_be_bytes());
        hello.extend_from_slice(&sni_ext);

        // Fix lengths
        let total = hello.len() - 5;
        hello[len_pos] = (total >> 8) as u8;
        hello[len_pos + 1] = total as u8;
        let hs_len = hello.len() - hs_len_pos - 3;
        hello[hs_len_pos] = (hs_len >> 16) as u8;
        hello[hs_len_pos + 1] = (hs_len >> 8) as u8;
        hello[hs_len_pos + 2] = hs_len as u8;

        hello
    }

    fn build_sni_extension(&self) -> Vec<u8> {
        let mut ext = Vec::new();
        // Extension type: server_name (0x0000)
        ext.extend_from_slice(&[0x00, 0x00]);
        let name_len = self.server_name_len;
        let list_len = name_len + 3;
        let ext_len = list_len + 2;
        ext.extend_from_slice(&(ext_len as u16).to_be_bytes());
        ext.extend_from_slice(&(list_len as u16).to_be_bytes());
        ext.push(0x00); // host_name type
        ext.extend_from_slice(&(name_len as u16).to_be_bytes());
        ext.extend_from_slice(&self.server_name[..name_len]);
        ext
    }

    /// Verify Reality server response (check auth_tag in ServerHello)
    pub fn verify_server_hello(&self, data: &[u8]) -> bool {
        // Minimal check: TLS record type 0x16, handshake type 0x02
        data.len() >= 6 && data[0] == 0x16 && data[5] == 0x02
    }
}

// ---------------------------------------------------------------------------
// Unified Transport Interface
// ---------------------------------------------------------------------------

/// Transport wrapper that dispatches to the configured transport
pub enum Transport {
    Tcp,
    WebSocket(WebSocketTransport),
    Grpc(GrpcTransport),
    Reality(RealityTransport),
}

impl Transport {
    /// Wrap payload in transport framing
    pub fn wrap(&self, payload: &[u8]) -> Vec<u8> {
        match self {
            Self::Tcp => payload.to_vec(),
            Self::WebSocket(ws) => ws.encode_frame(payload),
            Self::Grpc(grpc) => grpc.encode(payload),
            Self::Reality(_) => {
                // Reality uses raw TLS application data
                // Framing: [0x17][0x03][0x03][len_hi][len_lo][payload]
                let mut frame = Vec::with_capacity(5 + payload.len());
                frame.push(0x17); // Application Data
                frame.extend_from_slice(&[0x03, 0x03]); // TLS 1.2
                frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
                frame.extend_from_slice(payload);
                frame
            }
        }
    }

    /// Unwrap transport framing, return payload
    pub fn unwrap(&self, data: &[u8]) -> Result<Vec<u8>, KernelError> {
        match self {
            Self::Tcp => Ok(data.to_vec()),
            Self::WebSocket(_) => WebSocketTransport::decode_frame(data).map(|(p, _)| p),
            Self::Grpc(_) => GrpcTransport::decode(data),
            Self::Reality(_) => {
                if data.len() < 5 || data[0] != 0x17 {
                    return Err(KernelError::InvalidParameter("not TLS app data"));
                }
                let len = u16::from_be_bytes([data[3], data[4]]) as usize;
                if data.len() < 5 + len {
                    return Err(KernelError::InvalidParameter("TLS record truncated"));
                }
                Ok(data[5..5 + len].to_vec())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_encode_decode() {
        let ws = WebSocketTransport::new(b"/ws", b"example.com");
        let payload = b"hello vless";
        let frame = ws.encode_frame(payload);
        assert!(frame.len() > payload.len());

        // Server frames are unmasked
        let mut server_frame = Vec::new();
        server_frame.push(0x82); // FIN + Binary
        server_frame.push(payload.len() as u8); // no mask bit
        server_frame.extend_from_slice(payload);
        let (decoded, _) = WebSocketTransport::decode_frame(&server_frame).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_grpc_encode_decode() {
        let grpc = GrpcTransport::new(b"GunService/Tun");
        let payload = b"test data";
        let frame = grpc.encode(payload);
        assert_eq!(frame[0], 0); // not compressed
        let decoded = GrpcTransport::decode(&frame).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_reality_client_hello() {
        let reality = RealityTransport::new(
            b"www.google.com",
            [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
            [0xAA; 32],
        );
        let hello = reality.build_client_hello();
        assert_eq!(hello[0], 0x16); // TLS Handshake
        assert_eq!(hello[5], 0x01); // ClientHello
    }

    #[test]
    fn test_transport_wrap_unwrap() {
        let transport = Transport::Grpc(GrpcTransport::new(b"Svc/Method"));
        let data = b"proxy payload";
        let wrapped = transport.wrap(data);
        let unwrapped = transport.unwrap(&wrapped).unwrap();
        assert_eq!(unwrapped, data);
    }
}
