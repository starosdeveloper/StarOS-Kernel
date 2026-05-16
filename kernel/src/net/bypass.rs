//! Native Bypass — Автоматический обход блокировок
//!
//! Обеспечивает нативный доступ ко всем заблокированным сервисам без
//! какой-либо настройки со стороны пользователя.
//!
//! Принцип работы:
//! 1. При загрузке ОС автоматически активируется VLESS+Reality туннель
//! 2. Трафик к заблокированным ресурсам прозрачно маршрутизируется через туннель
//! 3. Остальной трафик идёт напрямую (split tunneling)
//! 4. DPI не может обнаружить туннель (Reality = легитимный TLS)
//!
//! Для пользователя: всё просто работает. Никаких VPN, прокси, настроек.

use crate::net::vless::{self, VlessConfig, Uuid, TransportType, FlowType};
use crate::error::KernelError;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Bypass State
// ---------------------------------------------------------------------------

/// Bypass operational mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BypassMode {
    /// Disabled (user explicitly turned off)
    Disabled = 0,
    /// Auto — route only blocked traffic through tunnel
    Auto = 1,
    /// Full — route ALL traffic through tunnel
    Full = 2,
}

static BYPASS_MODE: AtomicU8 = AtomicU8::new(BypassMode::Auto as u8);
static BYPASS_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Built-in server pool (rotated automatically)
/// In production these would be fetched from a secure CDN
struct ServerEntry {
    addr: &'static str,
    port: u16,
    uuid: &'static str,
    sni: &'static str,
}

/// Server pool — multiple servers for redundancy
static SERVERS: &[ServerEntry] = &[
    ServerEntry { addr: "gateway-1.staros.network", port: 443, uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890", sni: "www.microsoft.com" },
    ServerEntry { addr: "gateway-2.staros.network", port: 443, uuid: "b2c3d4e5-f6a7-8901-bcde-f12345678901", sni: "www.apple.com" },
    ServerEntry { addr: "gateway-3.staros.network", port: 443, uuid: "c3d4e5f6-a7b8-9012-cdef-123456789012", sni: "www.google.com" },
];

static CURRENT_SERVER: AtomicU8 = AtomicU8::new(0);

// ---------------------------------------------------------------------------
// Domain Routing Rules (split tunneling)
// ---------------------------------------------------------------------------

/// Domains that should be routed through the tunnel
/// These are checked against DNS queries and connection destinations
static BYPASS_DOMAINS: &[&str] = &[
    // Social media
    "instagram.com",
    "cdninstagram.com",
    "twitter.com",
    "x.com",
    "twimg.com",
    "facebook.com",
    "fbcdn.net",
    "threads.net",
    "linkedin.com",

    // Messengers
    "discord.com",
    "discord.gg",
    "discordapp.com",
    "signal.org",
    "whispersystems.org",

    // Media
    "youtube.com",
    "googlevideo.com",
    "ytimg.com",
    "spotify.com",
    "scdn.co",
    "soundcloud.com",

    // AI & Tech
    "openai.com",
    "chatgpt.com",
    "anthropic.com",
    "claude.ai",
    "notion.so",
    "notion.com",
    "medium.com",

    // Dev tools
    "github.com",
    "githubusercontent.com",
    "docker.com",
    "docker.io",
    "npmjs.com",
    "pypi.org",

    // News & Info
    "bbc.com",
    "bbc.co.uk",
    "reuters.com",
];

/// IP ranges that should be routed through tunnel (CIDR)
/// These cover known blocked IP ranges
static BYPASS_IP_RANGES: &[(&str, u8)] = &[
    // Discord
    ("162.159.128.0", 17),
    // Cloudflare (partial, for blocked sites)
    ("104.16.0.0", 12),
    // Twitter/X
    ("104.244.42.0", 24),
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize native bypass at boot time.
/// Called automatically by the kernel during startup.
/// No user interaction required.
pub fn init() -> Result<(), KernelError> {
    let mode = current_mode();
    if mode == BypassMode::Disabled {
        return Ok(());
    }

    // Select best server (round-robin with fallback)
    let server_idx = CURRENT_SERVER.load(Ordering::Relaxed) as usize % SERVERS.len();
    let server = &SERVERS[server_idx];

    // Parse UUID
    let uuid = Uuid::parse(server.uuid)?;

    // Build config — Reality transport for maximum stealth
    let mut config = VlessConfig {
        server_addr: [0; 256],
        server_addr_len: server.addr.len(),
        server_port: server.port,
        uuid,
        transport: TransportType::Reality,
        flow: FlowType::XtlsVision,
        sni: [0; 256],
        sni_len: server.sni.len(),
        enabled: true,
    };
    config.server_addr[..server.addr.len()].copy_from_slice(server.addr.as_bytes());
    config.sni[..server.sni.len()].copy_from_slice(server.sni.as_bytes());

    // Configure and start tunnel
    vless::tunnel().configure(config)?;
    vless::tunnel().start()?;

    BYPASS_ACTIVE.store(true, Ordering::Release);
    Ok(())
}

/// Check if a domain should be routed through the tunnel
pub fn should_bypass(domain: &str) -> bool {
    if !BYPASS_ACTIVE.load(Ordering::Acquire) {
        return false;
    }

    let mode = current_mode();
    if mode == BypassMode::Full {
        return true;
    }

    // Check against bypass list
    let domain_lower = domain.as_bytes();
    for &blocked in BYPASS_DOMAINS {
        if domain_ends_with(domain_lower, blocked.as_bytes()) {
            return true;
        }
    }
    false
}

/// Check if an IP should be routed through the tunnel
pub fn should_bypass_ip(ip: &[u8; 4]) -> bool {
    if !BYPASS_ACTIVE.load(Ordering::Acquire) {
        return false;
    }
    if current_mode() == BypassMode::Full {
        return true;
    }

    let ip_u32 = u32::from_be_bytes(*ip);
    for &(range_str, prefix_len) in BYPASS_IP_RANGES {
        if let Some(range_ip) = parse_ipv4(range_str) {
            let mask = if prefix_len >= 32 { 0xFFFF_FFFF } else { !((1u32 << (32 - prefix_len)) - 1) };
            if (ip_u32 & mask) == (range_ip & mask) {
                return true;
            }
        }
    }
    false
}

/// Set bypass mode
pub fn set_mode(mode: BypassMode) {
    BYPASS_MODE.store(mode as u8, Ordering::Release);
    if mode == BypassMode::Disabled {
        vless::tunnel().stop();
        BYPASS_ACTIVE.store(false, Ordering::Release);
    }
}

/// Get current mode
pub fn current_mode() -> BypassMode {
    match BYPASS_MODE.load(Ordering::Acquire) {
        0 => BypassMode::Disabled,
        1 => BypassMode::Auto,
        _ => BypassMode::Full,
    }
}

/// Switch to next server (on connection failure)
pub fn rotate_server() -> Result<(), KernelError> {
    let next = (CURRENT_SERVER.fetch_add(1, Ordering::Relaxed) + 1) as usize % SERVERS.len();
    vless::tunnel().stop();
    CURRENT_SERVER.store(next as u8, Ordering::Relaxed);
    init()
}

/// Check if bypass is currently active
pub fn is_active() -> bool {
    BYPASS_ACTIVE.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn domain_ends_with(domain: &[u8], suffix: &[u8]) -> bool {
    if domain.len() < suffix.len() {
        return false;
    }
    let start = domain.len() - suffix.len();
    // Must match at domain boundary (start of string or preceded by '.')
    if start > 0 && domain[start - 1] != b'.' {
        // Check if it's an exact match
        if start != 0 {
            return false;
        }
    }
    domain[start..].eq_ignore_ascii_case(suffix)
}

fn parse_ipv4(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    let mut octets = [0u8; 4];
    let mut octet_idx = 0;
    let mut current: u16 = 0;

    for &b in bytes {
        if b == b'.' {
            if octet_idx >= 3 || current > 255 { return None; }
            octets[octet_idx] = current as u8;
            octet_idx += 1;
            current = 0;
        } else if b >= b'0' && b <= b'9' {
            current = current * 10 + (b - b'0') as u16;
        } else {
            return None;
        }
    }
    if octet_idx != 3 || current > 255 { return None; }
    octets[3] = current as u8;
    Some(u32::from_be_bytes(octets))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_matching() {
        assert!(domain_ends_with(b"www.instagram.com", b"instagram.com"));
        assert!(domain_ends_with(b"instagram.com", b"instagram.com"));
        assert!(!domain_ends_with(b"notinstagram.com", b"instagram.com"));
        assert!(domain_ends_with(b"api.discord.com", b"discord.com"));
    }

    #[test]
    fn test_parse_ipv4() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some(0xC0A80101));
        assert_eq!(parse_ipv4("10.0.0.1"), Some(0x0A000001));
        assert_eq!(parse_ipv4("invalid"), None);
    }

    #[test]
    fn test_bypass_mode() {
        set_mode(BypassMode::Auto);
        assert_eq!(current_mode(), BypassMode::Auto);
        set_mode(BypassMode::Full);
        assert_eq!(current_mode(), BypassMode::Full);
    }

    #[test]
    fn test_should_bypass() {
        BYPASS_ACTIVE.store(true, Ordering::Release);
        BYPASS_MODE.store(BypassMode::Auto as u8, Ordering::Release);

        assert!(should_bypass("www.instagram.com"));
        assert!(should_bypass("discord.com"));
        assert!(should_bypass("chatgpt.com"));
        assert!(!should_bypass("yandex.ru"));
        assert!(!should_bypass("vk.com"));
    }

    #[test]
    fn test_ip_bypass() {
        BYPASS_ACTIVE.store(true, Ordering::Release);
        BYPASS_MODE.store(BypassMode::Auto as u8, Ordering::Release);

        // Discord IP range
        assert!(should_bypass_ip(&[162, 159, 130, 1]));
        // Random IP - not bypassed
        assert!(!should_bypass_ip(&[192, 168, 1, 1]));
    }
}
