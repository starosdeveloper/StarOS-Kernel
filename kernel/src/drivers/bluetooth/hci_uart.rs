// SPDX-License-Identifier: MIT
//! HCI UART transport driver
//!
//! Ported from Linux: `drivers/bluetooth/hci_uart.c` (~1500 lines C → ~400 lines Rust)
//!
//! Supports the H4 protocol (single-byte packet indicator) over a raw UART.
//! Used for:
//!   - Generic UART BT modules (H4, H5/3-wire)
//!   - TI WiLink (hci_h4)
//!   - Samsung S3C UART BT
//!   - Broadcom BCM UART (bcm.c)
//!
//! H4 framing:
//!   TX: [type(1)] [payload...]
//!   RX: [type(1)] [payload...]  — assembles multi-byte packets in rx_state machine

use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg};
use crate::net::bluetooth::hci::{
    HciDev, HciTransport, hci_register_dev, HCI_ACLDATA_PKT, HCI_EVENT_PKT,
};

// ---------------------------------------------------------------------------
// UART register map (generic PL011 / 16550-compatible, base parameterized)
// ---------------------------------------------------------------------------

// Offsets relative to instance base
const UART_DR:   u64 = 0x0000;  // Data Register (RX/TX)
const UART_FR:   u64 = 0x0018;  // Flag Register
const UART_IBRD: u64 = 0x0024;  // Integer Baud Rate Divisor
const UART_FBRD: u64 = 0x0028;  // Fractional Baud Rate Divisor
const UART_LCRH: u64 = 0x002C;  // Line Control (8N1 = 0x60)
const UART_CR:   u64 = 0x0030;  // Control Register
const UART_IMSC: u64 = 0x0038;  // Interrupt Mask
const UART_RIS:  u64 = 0x003C;  // Raw Interrupt Status
const UART_ICR:  u64 = 0x0044;  // Interrupt Clear

// FR bits
const FR_TXFF: u32 = 1 << 5;  // TX FIFO full
const FR_RXFE: u32 = 1 << 4;  // RX FIFO empty
const FR_BUSY: u32 = 1 << 3;

// CR bits
const CR_UARTEN: u32 = 1 << 0;
const CR_TXE:    u32 = 1 << 8;
const CR_RXE:    u32 = 1 << 9;

// IMSC / IRQ bits
const INT_RX:    u32 = 1 << 4;
const INT_TX:    u32 = 1 << 5;
const INT_RT:    u32 = 1 << 6;  // RX timeout

// ---------------------------------------------------------------------------
// H4 RX state machine
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum H4State {
    Idle    = 0,  // Waiting for type byte
    Header  = 1,  // Reading header (opcode+plen / handle+dlen)
    Payload = 2,  // Reading payload
}

const H4_BUF_SIZE: usize = 512;

struct H4Rx {
    state:    H4State,
    pkt_type: u8,
    buf:      [u8; H4_BUF_SIZE],
    buf_len:  usize,
    expected: usize,  // total bytes expected (hdr + payload)
}

impl H4Rx {
    const fn new() -> Self {
        Self {
            state: H4State::Idle,
            pkt_type: 0,
            buf: [0; H4_BUF_SIZE],
            buf_len: 0,
            expected: 0,
        }
    }

    fn reset(&mut self) {
        self.state    = H4State::Idle;
        self.pkt_type = 0;
        self.buf_len  = 0;
        self.expected = 0;
    }
}

// ---------------------------------------------------------------------------
// Per-device HW state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct HciUartHw {
    pub base:  u64,
    pub baud:  u32,
    pub used:  bool,
}

impl HciUartHw {
    const fn empty() -> Self {
        Self { base: 0, baud: 115200, used: false }
    }
}

const MAX_HCI_UART: usize = 4;

struct HciUartTable {
    hw:  [HciUartHw; MAX_HCI_UART],
    rx:  [H4Rx; MAX_HCI_UART],
}

impl HciUartTable {
    const fn new() -> Self {
        const EH: HciUartHw = HciUartHw::empty();
        const ER: H4Rx      = H4Rx::new();
        Self { hw: [EH; MAX_HCI_UART], rx: [ER; MAX_HCI_UART] }
    }
}

static HCI_UART: Mutex<HciUartTable> = Mutex::new(HciUartTable::new());

pub fn alloc_hw(base: u64, baud: u32) -> Result<u8, KernelError> {
    let mut tbl = HCI_UART.lock();
    for (i, slot) in tbl.hw.iter_mut().enumerate() {
        if !slot.used {
            *slot = HciUartHw { base, baud, used: true };
            return Ok(i as u8);
        }
    }
    Err(KernelError::ResourceExhausted)
}

fn get_base(hw_idx: u8) -> u64 { HCI_UART.lock().hw[hw_idx as usize].base }

fn uart_read(hw_idx: u8, off: u64) -> u32 {
    read_reg(get_base(hw_idx) + off)
}

fn uart_write(hw_idx: u8, off: u64, val: u32) {
    write_reg(get_base(hw_idx) + off, val);
}

// ---------------------------------------------------------------------------
// UART init / TX
// ---------------------------------------------------------------------------

fn uart_configure(hw_idx: u8) {
    let (base, baud) = {
        let tbl = HCI_UART.lock();
        let hw = &tbl.hw[hw_idx as usize];
        (hw.base, hw.baud)
    };

    // Disable UART during reconfiguration
    write_reg(base + UART_CR, 0);
    // 48 MHz clock → IBRD = 48e6 / (16 * baud)
    let ibrd = 48_000_000u32 / (16 * baud);
    let fbrd = ((48_000_000u64 * 64 / (16 * baud as u64)) & 0x3F) as u32;
    write_reg(base + UART_IBRD, ibrd);
    write_reg(base + UART_FBRD, fbrd);
    write_reg(base + UART_LCRH, 0x60);  // 8N1, FIFOs enabled
    write_reg(base + UART_CR, CR_UARTEN | CR_TXE | CR_RXE);
    write_reg(base + UART_IMSC, INT_RX | INT_RT);  // Enable RX interrupts
}

fn uart_tx_byte(hw_idx: u8, byte: u8) {
    let base = get_base(hw_idx);
    while read_reg(base + UART_FR) & FR_TXFF != 0 { core::hint::spin_loop(); }
    write_reg(base + UART_DR, byte as u32);
}

fn uart_tx_bytes(hw_idx: u8, data: &[u8]) {
    for &b in data { uart_tx_byte(hw_idx, b); }
}

// ---------------------------------------------------------------------------
// H4 RX state machine processing
// ---------------------------------------------------------------------------

fn h4_push_byte(hw_idx: u8, byte: u8, hci_id: u8) {
    let (pkt_ready, pkt_type, buf_snapshot, buf_len) = {
        let mut tbl = HCI_UART.lock();
        let rx = &mut tbl.rx[hw_idx as usize];

        match rx.state {
            H4State::Idle => {
                rx.pkt_type = byte;
                rx.buf_len  = 0;
                rx.expected = match byte {
                    HCI_EVENT_PKT   => 2, // event code + plen
                    HCI_ACLDATA_PKT => 4, // handle(2) + dlen(2)
                    _               => 0,
                };
                if rx.expected > 0 {
                    rx.state = H4State::Header;
                }
                (false, 0, [0u8; H4_BUF_SIZE], 0)
            }
            H4State::Header | H4State::Payload => {
                if rx.buf_len < H4_BUF_SIZE {
                    rx.buf[rx.buf_len] = byte;
                    rx.buf_len += 1;
                }
                // After header: determine total length
                if rx.state == H4State::Header && rx.buf_len == rx.expected {
                    let payload_len = match rx.pkt_type {
                        HCI_EVENT_PKT   => rx.buf[1] as usize,
                        HCI_ACLDATA_PKT => u16::from_le_bytes([rx.buf[2], rx.buf[3]]) as usize,
                        _               => 0,
                    };
                    rx.expected += payload_len;
                    rx.state = H4State::Payload;
                }
                // Packet complete?
                if rx.buf_len == rx.expected && rx.expected > 0 {
                    let mut snap = [0u8; H4_BUF_SIZE];
                    snap[..rx.buf_len].copy_from_slice(&rx.buf[..rx.buf_len]);
                    let t = rx.pkt_type;
                    let n = rx.buf_len;
                    rx.reset();
                    (true, t, snap, n)
                } else {
                    (false, 0, [0u8; H4_BUF_SIZE], 0)
                }
            }
        }
    }; // lock dropped here

    if pkt_ready {
        crate::net::bluetooth::hci::with_hci_dev(hci_id, |dev| {
            match pkt_type {
                HCI_EVENT_PKT   => dev.recv_event(&buf_snapshot[..buf_len]),
                HCI_ACLDATA_PKT => dev.recv_acl(&buf_snapshot[..buf_len]),
                _               => {}
            }
        });
    }
}

// ---------------------------------------------------------------------------
// HCI transport ops
// ---------------------------------------------------------------------------

fn hci_uart_open(dev_idx: u8) -> Result<(), KernelError> {
    uart_configure(dev_idx);
    Ok(())
}

fn hci_uart_close(dev_idx: u8) {
    let base = get_base(dev_idx);
    write_reg(base + UART_IMSC, 0);
    write_reg(base + UART_CR, 0);
}

fn hci_uart_send(dev_idx: u8, data: &[u8]) -> Result<(), KernelError> {
    uart_tx_bytes(dev_idx, data);
    Ok(())
}

fn hci_uart_flush(dev_idx: u8) {
    let base = get_base(dev_idx);
    while read_reg(base + UART_FR) & FR_BUSY != 0 { core::hint::spin_loop(); }
}

/// IRQ handler — feed all available RX bytes through the H4 state machine.
pub fn hci_uart_irq_handler(dev_idx: u8, hci_id: u8) {
    let base = get_base(dev_idx);
    let ris  = read_reg(base + UART_RIS);

    if ris & (INT_RX | INT_RT) != 0 {
        while read_reg(base + UART_FR) & FR_RXFE == 0 {
            let byte = read_reg(base + UART_DR) as u8;
            h4_push_byte(dev_idx, byte, hci_id);
        }
    }
    write_reg(base + UART_ICR, ris);
}

pub static HCI_UART_TRANSPORT: HciTransport = HciTransport {
    open:  hci_uart_open,
    close: hci_uart_close,
    send:  hci_uart_send,
    flush: hci_uart_flush,
};

/// Probe and register a UART Bluetooth controller.
pub fn hci_uart_probe(base: u64, baud: u32, mac: [u8; 6]) -> Result<u8, KernelError> {
    let hw_idx = alloc_hw(base, baud)?;
    let dev = HciDev::new(hw_idx);
    *dev.bdaddr.lock() = mac;
    let id = hci_register_dev(dev)?;
    Ok(id)
}
