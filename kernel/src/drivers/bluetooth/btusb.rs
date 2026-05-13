// SPDX-License-Identifier: MIT
//! USB Bluetooth driver (btusb)
//!
//! Ported from Linux: `drivers/bluetooth/btusb.c` (~3500 lines C → ~400 lines Rust)
//!
//! Supports any USB Bluetooth dongle following the Bluetooth USB transport
//! spec (USB class 0xE0, subclass 0x01, protocol 0x01).
//!
//! USB endpoints used:
//!   EP0  — Control (HCI commands)
//!   EP1 IN  — Interrupt (HCI events)
//!   EP2 IN  — Bulk IN (ACL data)
//!   EP2 OUT — Bulk OUT (ACL data)
//!   EP3 IN / EP4 OUT — Isoc (SCO audio, optional)
//!
//! We model the USB controller as MMIO registers (simplified — real USB
//! would go through a USB HC driver layer).

use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg, rmw_reg};
use crate::net::bluetooth::hci::{HciDev, HciTransport, hci_register_dev};

// ---------------------------------------------------------------------------
// USB HC MMIO stub  (simplified — maps USB control/bulk transfers to MMIO)
// ---------------------------------------------------------------------------

const USB_BT_BASE: u64 = 0x0080_0000;  // USB host controller base for BT port

const USB_CMD_REG:      u64 = USB_BT_BASE + 0x0000;
const USB_STATUS_REG:   u64 = USB_BT_BASE + 0x0004;
const USB_EP0_DATA:     u64 = USB_BT_BASE + 0x0010;  // Control EP0 data port
const USB_EP0_LEN:      u64 = USB_BT_BASE + 0x0014;
const USB_EP1_DATA:     u64 = USB_BT_BASE + 0x0020;  // Interrupt EP1 RX
const USB_EP1_STATUS:   u64 = USB_BT_BASE + 0x0024;
const USB_EP2_TX_DATA:  u64 = USB_BT_BASE + 0x0030;  // Bulk OUT
const USB_EP2_TX_LEN:   u64 = USB_BT_BASE + 0x0034;
const USB_EP2_RX_DATA:  u64 = USB_BT_BASE + 0x0040;  // Bulk IN
const USB_EP2_RX_LEN:   u64 = USB_BT_BASE + 0x0044;
const USB_IRQ_STATUS:   u64 = USB_BT_BASE + 0x0100;
const USB_IRQ_MASK:     u64 = USB_BT_BASE + 0x0104;
const USB_IRQ_CLEAR:    u64 = USB_BT_BASE + 0x0108;

const USB_CMD_RESET:    u32 = 1 << 0;
const USB_CMD_RUN:      u32 = 1 << 1;

const USB_ST_READY:     u32 = 1 << 0;
const USB_ST_EP0_DONE:  u32 = 1 << 1;
const USB_ST_EP2_DONE:  u32 = 1 << 2;

const USB_IRQ_EVENT:    u32 = 1 << 0;  // EP1 interrupt (HCI event)
const USB_IRQ_ACL_RX:   u32 = 1 << 1;  // EP2 bulk RX
const USB_IRQ_ACL_TX:   u32 = 1 << 2;  // EP2 bulk TX done

// ---------------------------------------------------------------------------
// Per-device HW state (index-based)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct BtUsbHw {
    pub base: u64,
    pub ready: bool,
    pub used:  bool,
}

impl BtUsbHw {
    const fn empty() -> Self {
        Self { base: 0, ready: false, used: false }
    }
}

const MAX_BTUSB: usize = 4;
static BTUSB_TABLE: Mutex<[BtUsbHw; MAX_BTUSB]> = Mutex::new([BtUsbHw::empty(); MAX_BTUSB]);

pub fn alloc_hw(base: u64) -> Result<u8, KernelError> {
    let mut tbl = BTUSB_TABLE.lock();
    for (i, slot) in tbl.iter_mut().enumerate() {
        if !slot.used {
            *slot = BtUsbHw { base, ready: false, used: true };
            return Ok(i as u8);
        }
    }
    Err(KernelError::ResourceExhausted)
}

fn chip_base(hw_idx: u8) -> u64 { BTUSB_TABLE.lock()[hw_idx as usize].base }
fn usb_read(hw_idx: u8, reg: u64) -> u32 { read_reg(chip_base(hw_idx) + reg - USB_BT_BASE) }
fn usb_write(hw_idx: u8, reg: u64, val: u32) { write_reg(chip_base(hw_idx) + reg - USB_BT_BASE, val); }

// ---------------------------------------------------------------------------
// USB control transfer (HCI command via EP0)
// ---------------------------------------------------------------------------

fn usb_hci_cmd(hw_idx: u8, data: &[u8]) -> Result<(), KernelError> {
    // Write length then data to EP0 FIFO
    usb_write(hw_idx, USB_EP0_LEN, data.len() as u32);
    for chunk in data.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        usb_write(hw_idx, USB_EP0_DATA, u32::from_le_bytes(w));
    }
    // Trigger control transfer
    rmw_reg(chip_base(hw_idx) + USB_CMD_REG - USB_BT_BASE, 0x04, 0x04);
    // Wait for EP0 done
    for _ in 0..5000 {
        if usb_read(hw_idx, USB_STATUS_REG) & USB_ST_EP0_DONE != 0 { return Ok(()); }
        core::hint::spin_loop();
    }
    Err(KernelError::Timeout)
}

// ---------------------------------------------------------------------------
// HCI transport ops
// ---------------------------------------------------------------------------

fn btusb_open(dev_idx: u8) -> Result<(), KernelError> {
    // Reset USB HC
    usb_write(dev_idx, USB_CMD_REG, USB_CMD_RESET);
    usb_write(dev_idx, USB_CMD_REG, USB_CMD_RUN);

    // Wait for device ready
    for _ in 0..50_000 {
        if usb_read(dev_idx, USB_STATUS_REG) & USB_ST_READY != 0 { break; }
        core::hint::spin_loop();
    }

    // Enable IRQs: events + ACL RX
    usb_write(dev_idx, USB_IRQ_MASK, USB_IRQ_EVENT | USB_IRQ_ACL_RX);
    BTUSB_TABLE.lock()[dev_idx as usize].ready = true;
    Ok(())
}

fn btusb_close(dev_idx: u8) {
    usb_write(dev_idx, USB_IRQ_MASK, 0);
    usb_write(dev_idx, USB_CMD_REG, USB_CMD_RESET);
    BTUSB_TABLE.lock()[dev_idx as usize].ready = false;
}

fn btusb_send(dev_idx: u8, data: &[u8]) -> Result<(), KernelError> {
    if data.is_empty() { return Ok(()); }
    match data[0] {
        crate::net::bluetooth::hci::HCI_COMMAND_PKT => usb_hci_cmd(dev_idx, &data[1..]),
        crate::net::bluetooth::hci::HCI_ACLDATA_PKT => {
            // Bulk OUT via EP2
            usb_write(dev_idx, USB_EP2_TX_LEN, (data.len() - 1) as u32);
            for chunk in data[1..].chunks(4) {
                let mut w = [0u8; 4];
                w[..chunk.len()].copy_from_slice(chunk);
                usb_write(dev_idx, USB_EP2_TX_DATA, u32::from_le_bytes(w));
            }
            Ok(())
        }
        _ => Err(KernelError::InvalidParameter("unknown packet type")),
    }
}

fn btusb_flush(_dev_idx: u8) {}

/// IRQ handler — read events from EP1 and ACL data from EP2.
pub fn btusb_irq_handler(dev_idx: u8, hci_id: u8) {
    let status = usb_read(dev_idx, USB_IRQ_STATUS);

    if status & USB_IRQ_EVENT != 0 {
        // Read HCI event packet from EP1
        let len = usb_read(dev_idx, USB_EP1_STATUS) as usize & 0xFF;
        let mut buf = [0u8; 256];
        for i in 0..(len / 4 + 1).min(64) {
            let w = usb_read(dev_idx, USB_EP1_DATA).to_le_bytes();
            let off = i * 4;
            if off < 256 { buf[off..(off + 4).min(256)].copy_from_slice(&w[..(256 - off).min(4)]); }
        }
        crate::net::bluetooth::hci::with_hci_dev(hci_id, |dev| {
            dev.recv_event(&buf[..len.min(256)]);
        });
    }

    if status & USB_IRQ_ACL_RX != 0 {
        let len = usb_read(dev_idx, USB_EP2_RX_LEN) as usize & 0xFFFF;
        let mut buf = [0u8; 1024];
        for i in 0..(len / 4 + 1).min(256) {
            let w = usb_read(dev_idx, USB_EP2_RX_DATA).to_le_bytes();
            let off = i * 4;
            if off + 4 <= 1024 { buf[off..off + 4].copy_from_slice(&w); }
        }
        crate::net::bluetooth::hci::with_hci_dev(hci_id, |dev| {
            dev.recv_acl(&buf[..len.min(1024)]);
        });
    }

    usb_write(dev_idx, USB_IRQ_CLEAR, status);
}

pub static BTUSB_TRANSPORT: HciTransport = HciTransport {
    open:  btusb_open,
    close: btusb_close,
    send:  btusb_send,
    flush: btusb_flush,
};

/// Probe and register a USB Bluetooth dongle.
pub fn btusb_probe(base: u64, mac: [u8; 6]) -> Result<u8, KernelError> {
    let hw_idx = alloc_hw(base)?;
    let dev = HciDev::new(hw_idx);
    *dev.bdaddr.lock() = mac;
    let id = hci_register_dev(dev)?;
    Ok(id)
}
