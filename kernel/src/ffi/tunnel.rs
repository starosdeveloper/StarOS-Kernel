//! Tunnel FFI — Kotlin/Native bindings for VLESS/XRAY tunnel control

use crate::net::vless::{self, VlessConfig, Uuid, TransportType, FlowType, TunnelState};

/// Configure VLESS tunnel
/// Returns 0 on success, error code on failure
#[no_mangle]
pub extern "C" fn staros_tunnel_configure(
    server_addr: *const u8,
    server_addr_len: usize,
    server_port: u16,
    uuid_str: *const u8,
    uuid_str_len: usize,
    transport: u8, // 0=tcp, 1=ws, 2=grpc, 3=reality
    sni: *const u8,
    sni_len: usize,
) -> i32 {
    if server_addr.is_null() || uuid_str.is_null() || server_addr_len == 0 {
        return -1;
    }
    let addr_slice = unsafe { core::slice::from_raw_parts(server_addr, server_addr_len.min(256)) };
    let uuid_slice = unsafe { core::slice::from_raw_parts(uuid_str, uuid_str_len.min(36)) };
    let uuid_str = core::str::from_utf8(uuid_slice).unwrap_or("");
    let uuid = match Uuid::parse(uuid_str) {
        Ok(u) => u,
        Err(_) => return -2,
    };
    let transport_type = match transport {
        0 => TransportType::Tcp,
        1 => TransportType::WebSocket,
        2 => TransportType::Grpc,
        3 => TransportType::Reality,
        _ => return -3,
    };
    let mut config = VlessConfig {
        server_addr: [0; 256],
        server_addr_len: addr_slice.len(),
        server_port,
        uuid,
        transport: transport_type,
        flow: FlowType::XtlsVision,
        sni: [0; 256],
        sni_len: 0,
        enabled: true,
    };
    config.server_addr[..addr_slice.len()].copy_from_slice(addr_slice);
    if !sni.is_null() && sni_len > 0 {
        let sni_slice = unsafe { core::slice::from_raw_parts(sni, sni_len.min(256)) };
        config.sni[..sni_slice.len()].copy_from_slice(sni_slice);
        config.sni_len = sni_slice.len();
    }
    match vless::tunnel().configure(config) {
        Ok(()) => 0,
        Err(_) => -4,
    }
}

/// Start the tunnel. Returns 0 on success.
#[no_mangle]
pub extern "C" fn staros_tunnel_start() -> i32 {
    match vless::tunnel().start() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Stop the tunnel.
#[no_mangle]
pub extern "C" fn staros_tunnel_stop() {
    vless::tunnel().stop();
}

/// Get tunnel state: 0=disconnected, 1=connecting, 2=handshake, 3=active, 4=closing
#[no_mangle]
pub extern "C" fn staros_tunnel_state() -> u8 {
    match vless::tunnel().state() {
        TunnelState::Disconnected => 0,
        TunnelState::Connecting => 1,
        TunnelState::Handshake => 2,
        TunnelState::Active => 3,
        TunnelState::Closing => 4,
    }
}

/// Check if tunnel is active
#[no_mangle]
pub extern "C" fn staros_tunnel_is_active() -> bool {
    vless::tunnel().is_active()
}

/// Get tunnel statistics
#[no_mangle]
pub extern "C" fn staros_tunnel_get_stats(out: *mut TunnelStatsFFI) -> i32 {
    if out.is_null() { return -1; }
    let (tx_bytes, rx_bytes, tx_pkts, rx_pkts) = vless::tunnel().stats();
    unsafe {
        (*out).bytes_tx = tx_bytes;
        (*out).bytes_rx = rx_bytes;
        (*out).packets_tx = tx_pkts;
        (*out).packets_rx = rx_pkts;
    }
    0
}

/// C-compatible tunnel statistics
#[repr(C)]
pub struct TunnelStatsFFI {
    pub bytes_tx: u64,
    pub bytes_rx: u64,
    pub packets_tx: u64,
    pub packets_rx: u64,
}
