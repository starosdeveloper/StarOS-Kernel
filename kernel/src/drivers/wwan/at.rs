// SPDX-License-Identifier: MIT
//! AT Command Transport
//!
//! Ported from Linux: `drivers/net/wwan/wwan_core.c` (AT port handling)
//!
//! Minimal Hayes AT command parser and session manager for
//! embedded modem drivers (no heap, no alloc).
//!
//! AT session lifecycle:
//!   open → send_cmd → recv_response (poll) → close

use spin::Mutex;
use crate::error::KernelError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const AT_MAX_CMD_LEN:  usize = 128;
pub const AT_MAX_RESP_LEN: usize = 256;
pub const AT_MAX_SESSIONS: usize = 4;

// ---------------------------------------------------------------------------
// AT response codes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtResponse {
    Ok,
    Error,
    CmeError(u32),
    CmsError(u32),
    Connect(u32),  // baud rate
    NoCarrier,
    Busy,
    NoDialtone,
    Timeout,
    Data([u8; AT_MAX_RESP_LEN], usize), // response body + length
}

// ---------------------------------------------------------------------------
// AT session state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtState {
    Closed,
    Idle,
    WaitingResponse,
    DataMode,
}

// ---------------------------------------------------------------------------
// AT command buffer
// ---------------------------------------------------------------------------

pub struct AtCmd {
    pub buf: [u8; AT_MAX_CMD_LEN],
    pub len: usize,
}

impl AtCmd {
    pub const fn empty() -> Self {
        Self { buf: [0u8; AT_MAX_CMD_LEN], len: 0 }
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self, KernelError> {
        if b.len() > AT_MAX_CMD_LEN {
            return Err(KernelError::InvalidParameter(""));
        }
        let mut cmd = Self::empty();
        cmd.buf[..b.len()].copy_from_slice(b);
        cmd.len = b.len();
        Ok(cmd)
    }

    pub fn as_str(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

// ---------------------------------------------------------------------------
// AT session
// ---------------------------------------------------------------------------

pub struct AtSession {
    pub dev_idx: u8,
    pub state:   AtState,
    pub rx_buf:  [u8; AT_MAX_RESP_LEN],
    pub rx_len:  usize,
}

impl AtSession {
    pub const fn new(dev_idx: u8) -> Self {
        Self {
            dev_idx,
            state: AtState::Closed,
            rx_buf: [0u8; AT_MAX_RESP_LEN],
            rx_len: 0,
        }
    }

    /// Append a byte received from the modem UART.
    pub fn push_byte(&mut self, b: u8) {
        if self.rx_len < AT_MAX_RESP_LEN {
            self.rx_buf[self.rx_len] = b;
            self.rx_len += 1;
        }
    }

    /// Parse the accumulated RX buffer for a final response code.
    pub fn try_parse_response(&mut self) -> Option<AtResponse> {
        let buf = &self.rx_buf[..self.rx_len];
        // Check extended error codes BEFORE plain ERROR (they contain "ERROR" as substring)
        // Look for "+CME ERROR: <n>"
        if let Some(pos) = find_subslice(buf, b"+CME ERROR: ") {
            let code = parse_decimal(&buf[pos + 12..]).unwrap_or(0);
            self.rx_len = 0;
            return Some(AtResponse::CmeError(code));
        }
        // Look for "+CMS ERROR: <n>"
        if let Some(pos) = find_subslice(buf, b"+CMS ERROR: ") {
            let code = parse_decimal(&buf[pos + 12..]).unwrap_or(0);
            self.rx_len = 0;
            return Some(AtResponse::CmsError(code));
        }
        // Look for "OK\r\n"
        if buf.windows(2).any(|w| w == b"OK") {
            self.rx_len = 0;
            return Some(AtResponse::Ok);
        }
        // Look for plain "ERROR"
        if buf.windows(5).any(|w| w == b"ERROR") {
            self.rx_len = 0;
            return Some(AtResponse::Error);
        }
        // Look for "NO CARRIER"
        if buf.windows(10).any(|w| w == b"NO CARRIER") {
            self.rx_len = 0;
            return Some(AtResponse::NoCarrier);
        }
        // Look for "BUSY"
        if buf.windows(4).any(|w| w == b"BUSY") {
            self.rx_len = 0;
            return Some(AtResponse::Busy);
        }
        // Look for "CONNECT <baud>"
        if let Some(pos) = find_subslice(buf, b"CONNECT") {
            let baud = parse_decimal(&buf[pos + 7..]).unwrap_or(0);
            self.rx_len = 0;
            return Some(AtResponse::Connect(baud));
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Session table
// ---------------------------------------------------------------------------

pub struct AtSessionTable {
    pub sessions: [AtSession; AT_MAX_SESSIONS],
}

impl AtSessionTable {
    pub const fn new() -> Self {
        Self {
            sessions: [
                AtSession { dev_idx: 0, state: AtState::Closed, rx_buf: [0u8; AT_MAX_RESP_LEN], rx_len: 0 },
                AtSession { dev_idx: 1, state: AtState::Closed, rx_buf: [0u8; AT_MAX_RESP_LEN], rx_len: 0 },
                AtSession { dev_idx: 2, state: AtState::Closed, rx_buf: [0u8; AT_MAX_RESP_LEN], rx_len: 0 },
                AtSession { dev_idx: 3, state: AtState::Closed, rx_buf: [0u8; AT_MAX_RESP_LEN], rx_len: 0 },
            ],
        }
    }
}

pub static AT_SESSIONS: Mutex<AtSessionTable> = Mutex::new(AtSessionTable::new());

/// Open an AT session for a modem device.
pub fn at_open(dev_idx: u8) -> Result<(), KernelError> {
    let mut tbl = AT_SESSIONS.lock();
    let idx = dev_idx as usize;
    if idx >= AT_MAX_SESSIONS {
        return Err(KernelError::InvalidParameter(""));
    }
    tbl.sessions[idx].dev_idx = dev_idx;
    tbl.sessions[idx].state = AtState::Idle;
    tbl.sessions[idx].rx_len = 0;
    Ok(())
}

/// Send an AT command via the modem's UART MMIO.
pub fn at_send_cmd(dev_idx: u8, cmd: &AtCmd) -> Result<(), KernelError> {
    use crate::drivers::clk::mmio::write_reg;
    use super::qmi_wwan::qmi_wwan_base;

    let base = qmi_wwan_base(dev_idx);

    {
        let mut tbl = AT_SESSIONS.lock();
        let idx = dev_idx as usize;
        if idx >= AT_MAX_SESSIONS { return Err(KernelError::InvalidParameter("")); }
        if tbl.sessions[idx].state != AtState::Idle {
            return Err(KernelError::OperationFailed);
        }
        tbl.sessions[idx].state = AtState::WaitingResponse;
    }

    // Write AT command to UART TX FIFO at base + 0x20
    let at_base = base + 0x20;
    for &b in cmd.as_str() {
        write_reg(at_base, b as u32);
    }
    // Write CR+LF
    write_reg(at_base, b'\r' as u32);
    write_reg(at_base, b'\n' as u32);
    Ok(())
}

/// Feed a received byte from modem UART IRQ into the session.
pub fn at_recv_byte(dev_idx: u8, byte: u8) -> Option<AtResponse> {
    let mut tbl = AT_SESSIONS.lock();
    let idx = dev_idx as usize;
    if idx >= AT_MAX_SESSIONS { return None; }
    tbl.sessions[idx].push_byte(byte);
    if tbl.sessions[idx].state == AtState::WaitingResponse {
        let resp = tbl.sessions[idx].try_parse_response();
        if resp.is_some() {
            tbl.sessions[idx].state = AtState::Idle;
        }
        return resp;
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_decimal(buf: &[u8]) -> Option<u32> {
    let mut n: u32 = 0;
    let mut found = false;
    for &b in buf {
        if b >= b'0' && b <= b'9' {
            n = n.wrapping_mul(10).wrapping_add((b - b'0') as u32);
            found = true;
        } else if found {
            break;
        }
    }
    if found { Some(n) } else { None }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_at_parse_ok() {
        let mut sess = AtSession::new(0);
        sess.state = AtState::WaitingResponse;
        for b in b"OK\r\n" { sess.push_byte(*b); }
        assert_eq!(sess.try_parse_response(), Some(AtResponse::Ok));
    }

    #[test]
    fn test_at_parse_error() {
        let mut sess = AtSession::new(0);
        sess.state = AtState::WaitingResponse;
        for b in b"ERROR\r\n" { sess.push_byte(*b); }
        assert_eq!(sess.try_parse_response(), Some(AtResponse::Error));
    }

    #[test]
    fn test_at_parse_cme_error() {
        let mut sess = AtSession::new(0);
        sess.state = AtState::WaitingResponse;
        for b in b"+CME ERROR: 10\r\n" { sess.push_byte(*b); }
        assert_eq!(sess.try_parse_response(), Some(AtResponse::CmeError(10)));
    }

    #[test]
    fn test_at_parse_connect() {
        let mut sess = AtSession::new(0);
        sess.state = AtState::WaitingResponse;
        for b in b"CONNECT 115200\r\n" { sess.push_byte(*b); }
        assert_eq!(sess.try_parse_response(), Some(AtResponse::Connect(115200)));
    }

    #[test]
    fn test_at_parse_no_carrier() {
        let mut sess = AtSession::new(0);
        for b in b"NO CARRIER\r\n" { sess.push_byte(*b); }
        assert_eq!(sess.try_parse_response(), Some(AtResponse::NoCarrier));
    }

    #[test]
    fn test_at_parse_incomplete_returns_none() {
        let mut sess = AtSession::new(0);
        for b in b"AT+CIMI" { sess.push_byte(*b); }
        assert_eq!(sess.try_parse_response(), None);
    }

    #[test]
    fn test_at_cmd_from_bytes() {
        let cmd = AtCmd::from_bytes(b"AT+CGDCONT?").unwrap();
        assert_eq!(cmd.len, 11);
        assert_eq!(cmd.as_str(), b"AT+CGDCONT?");
    }

    #[test]
    fn test_at_cmd_too_long() {
        let long = [b'A'; AT_MAX_CMD_LEN + 1];
        assert!(AtCmd::from_bytes(&long).is_err());
    }

    #[test]
    fn test_at_open_sets_idle() {
        at_open(0).unwrap();
        let tbl = AT_SESSIONS.lock();
        assert_eq!(tbl.sessions[0].state, AtState::Idle);
    }
}
