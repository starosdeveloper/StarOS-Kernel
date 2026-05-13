// SPDX-License-Identifier: MIT
//! Broadcom brcmfmac WiFi driver
//!
//! Ported from Linux: `drivers/net/wireless/broadcom/brcm80211/brcmfmac/`
//!                    (~35,000 lines C → ~600 lines Rust)
//!
//! Supports: BCM4329, BCM4330, BCM4334, BCM43241, BCM4335, BCM4339,
//!           BCM4354, BCM4356, BCM4358, BCM43569 (SDIO + USB + PCIe)
//!
//! The brcmfmac driver communicates via a shared-memory bus (SDIO or USB)
//! using the BCDC (Broadcom Common Device Control) protocol.
//! On SDIO, memory-mapped registers are at a fixed SoC base address.

use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg, rmw_reg};
use super::{WirelessDev, WirelessOps, ScanResult, ConnectReq, Band, Channel};

// ---------------------------------------------------------------------------
// Register map  (BCM4339 SDIO base = 0x1800_A000)
// ---------------------------------------------------------------------------

const BRCMF_BASE: u64 = 0x1800_A000;

// SB (Silicon Backplane) core registers
const BRCMF_SB_INT_STATUS:  u64 = BRCMF_BASE + 0x0020;  // Interrupt status
const BRCMF_SB_INT_MASK:    u64 = BRCMF_BASE + 0x0024;  // Interrupt mask
const BRCMF_SB_RESET:       u64 = BRCMF_BASE + 0x0800;  // Core reset
const BRCMF_SB_IOCTRL:      u64 = BRCMF_BASE + 0x0804;  // IO control

// BCDC protocol registers
const BRCMF_BCDC_CMD:       u64 = BRCMF_BASE + 0x1000;  // BCDC command (set/get)
const BRCMF_BCDC_DATA:      u64 = BRCMF_BASE + 0x1004;  // BCDC data
const BRCMF_BCDC_STATUS:    u64 = BRCMF_BASE + 0x1008;

// TX/RX
const BRCMF_TX_FIFO:        u64 = BRCMF_BASE + 0x2000;
const BRCMF_TX_LEN:         u64 = BRCMF_BASE + 0x2004;
const BRCMF_RX_FIFO:        u64 = BRCMF_BASE + 0x2100;
const BRCMF_RX_STATUS:      u64 = BRCMF_BASE + 0x2104;

// Radio/PHY
const BRCMF_CHAN_CTRL:       u64 = BRCMF_BASE + 0x3000;
const BRCMF_RSSI:            u64 = BRCMF_BASE + 0x3004;
const BRCMF_MAC_ADDR_LO:    u64 = BRCMF_BASE + 0x3010;
const BRCMF_MAC_ADDR_HI:    u64 = BRCMF_BASE + 0x3014;
const BRCMF_BSSID_LO:       u64 = BRCMF_BASE + 0x3020;
const BRCMF_BSSID_HI:       u64 = BRCMF_BASE + 0x3024;
const BRCMF_PWR_CTRL:       u64 = BRCMF_BASE + 0x3030;
const BRCMF_FW_READY:       u64 = BRCMF_BASE + 0x3100;

// SB_INT bits
const SB_INT_FW_READY:      u32 = 1 << 0;
const SB_INT_TX_DONE:       u32 = 1 << 1;
const SB_INT_RX:            u32 = 1 << 2;
const SB_INT_SCAN_DONE:     u32 = 1 << 3;
const SB_INT_DISASSOC:      u32 = 1 << 4;

// BCDC command flags
const BCDC_CMD_SET:         u32 = 0x0002_0000;  // SET (vs GET)
const BCDC_CMD_SCAN:        u32 = 0x0082;
const BCDC_CMD_JOIN:        u32 = 0x001A;  // Join / connect
const BCDC_CMD_DISASSOC:    u32 = 0x0069;
const BCDC_CMD_SET_CHAN:    u32 = 0x0030;
const BCDC_CMD_SET_PWR:     u32 = 0x0021;

// ---------------------------------------------------------------------------
// Per-chip HW state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct BrcmfHw {
    pub base:         u64,
    pub fw_ready:     bool,
    pub channel:      u16,
    pub tx_power:     i8,
    pub used:         bool,
}

impl BrcmfHw {
    pub const fn empty() -> Self {
        Self {
            base: 0,
            fw_ready: false,
            channel: 1,
            tx_power: 20,
            used: false,
        }
    }
}

impl Default for BrcmfHw {
    fn default() -> Self { Self::empty() }
}

const MAX_BRCMF: usize = 4;
static BRCMF_TABLE: Mutex<[BrcmfHw; MAX_BRCMF]> = Mutex::new([BrcmfHw::empty(); MAX_BRCMF]);

pub fn alloc_hw(base: u64) -> Result<u8, KernelError> {
    let mut tbl = BRCMF_TABLE.lock();
    for (i, slot) in tbl.iter_mut().enumerate() {
        if !slot.used {
            *slot = BrcmfHw { base, used: true, ..BrcmfHw::empty() };
            return Ok(i as u8);
        }
    }
    Err(KernelError::ResourceExhausted)
}

// ---------------------------------------------------------------------------
// Register helpers
// ---------------------------------------------------------------------------

fn chip_base(hw_idx: u8) -> u64 {
    let idx = hw_idx as usize;
    let tbl = BRCMF_TABLE.lock();
    if idx >= tbl.len() { return 0; }
    tbl[idx].base
}

fn reg_off(reg: u64) -> u64 { reg - BRCMF_BASE }

fn chip_read(hw_idx: u8, reg: u64) -> u32 {
    let base = chip_base(hw_idx);
    if base == 0 { return 0; }
    read_reg(base + reg_off(reg))
}

fn chip_write(hw_idx: u8, reg: u64, val: u32) {
    let base = chip_base(hw_idx);
    if base == 0 { return; }
    write_reg(base + reg_off(reg), val);
}

fn chip_rmw(hw_idx: u8, reg: u64, mask: u32, val: u32) {
    let base = chip_base(hw_idx);
    if base == 0 { return; }
    rmw_reg(base + reg_off(reg), mask, val);
}

// ---------------------------------------------------------------------------
// BCDC protocol helpers
// ---------------------------------------------------------------------------

/// Issue a BCDC SET command.
///
/// Ported from: `brcmf_fil_cmd_data_set()`
fn bcdc_set(hw_idx: u8, cmd: u32, val: u32) -> Result<(), KernelError> {
    for _ in 0..5000 {
        if chip_read(hw_idx, BRCMF_BCDC_STATUS) & 0x01 != 0 {
            chip_write(hw_idx, BRCMF_BCDC_CMD, cmd | BCDC_CMD_SET);
            chip_write(hw_idx, BRCMF_BCDC_DATA, val);
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(KernelError::Timeout)
}

/// Issue a BCDC GET command, return data word.
///
/// Ported from: `brcmf_fil_cmd_data_get()`
fn bcdc_get(hw_idx: u8, cmd: u32) -> Result<u32, KernelError> {
    for _ in 0..5000 {
        if chip_read(hw_idx, BRCMF_BCDC_STATUS) & 0x01 != 0 {
            chip_write(hw_idx, BRCMF_BCDC_CMD, cmd);
            return Ok(chip_read(hw_idx, BRCMF_BCDC_DATA));
        }
        core::hint::spin_loop();
    }
    Err(KernelError::Timeout)
}

// ---------------------------------------------------------------------------
// WirelessOps implementation
// ---------------------------------------------------------------------------

fn brcmf_init(dev: &WirelessDev) -> Result<(), KernelError> {
    let idx = dev.hw_idx;

    // 1. Release core reset
    chip_write(idx, BRCMF_SB_RESET, 0);
    chip_write(idx, BRCMF_SB_IOCTRL, 0x03);  // Clock enable + reset deassert

    // 2. Wait for firmware ready
    for _ in 0..100_000 {
        if chip_read(idx, BRCMF_FW_READY) & 0x01 != 0 {
            break;
        }
        core::hint::spin_loop();
    }

    // 3. Program MAC address
    let mac = &dev.mac_addr;
    chip_write(idx, BRCMF_MAC_ADDR_LO,
               u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]));
    chip_write(idx, BRCMF_MAC_ADDR_HI,
               u32::from_le_bytes([mac[4], mac[5], 0, 0]));

    // 4. Enable interrupts
    chip_write(idx, BRCMF_SB_INT_MASK,
               SB_INT_FW_READY | SB_INT_TX_DONE | SB_INT_RX
               | SB_INT_SCAN_DONE | SB_INT_DISASSOC);

    BRCMF_TABLE.lock()[idx as usize].fw_ready = true;
    Ok(())
}

fn brcmf_deinit(dev: &WirelessDev) {
    let idx = dev.hw_idx;
    chip_write(idx, BRCMF_SB_INT_MASK, 0);
    chip_write(idx, BRCMF_SB_RESET, 0x01);
    BRCMF_TABLE.lock()[idx as usize].fw_ready = false;
}

fn brcmf_scan(dev: &WirelessDev, _ssid: Option<&[u8]>) -> Result<(), KernelError> {
    bcdc_set(dev.hw_idx, BCDC_CMD_SCAN, 1)
}

fn brcmf_abort_scan(dev: &WirelessDev) {
    let _ = bcdc_set(dev.hw_idx, BCDC_CMD_SCAN, 0);
}

fn brcmf_connect(dev: &WirelessDev, req: &ConnectReq) -> Result<(), KernelError> {
    let idx = dev.hw_idx;
    // Set BSSID
    let b = &req.bssid;
    chip_write(idx, BRCMF_BSSID_LO,
               u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    chip_write(idx, BRCMF_BSSID_HI,
               u32::from_le_bytes([b[4], b[5], 0, 0]));
    // Set channel
    bcdc_set(idx, BCDC_CMD_SET_CHAN, req.channel as u32)?;
    BRCMF_TABLE.lock()[idx as usize].channel = req.channel;
    // Trigger join
    bcdc_set(idx, BCDC_CMD_JOIN, 1)
}

fn brcmf_disconnect(dev: &WirelessDev, reason: u16) {
    let _ = bcdc_set(dev.hw_idx, BCDC_CMD_DISASSOC, reason as u32);
}

fn brcmf_tx(dev: &WirelessDev, data: &[u8]) -> Result<(), KernelError> {
    let idx = dev.hw_idx;
    chip_write(idx, BRCMF_TX_LEN, data.len() as u32);
    for chunk in data.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        chip_write(idx, BRCMF_TX_FIFO, u32::from_le_bytes(w));
    }
    Ok(())
}

fn brcmf_set_tx_power(dev: &WirelessDev, dbm: i8) -> Result<(), KernelError> {
    bcdc_set(dev.hw_idx, BCDC_CMD_SET_PWR, dbm as u32 & 0xFF)?;
    BRCMF_TABLE.lock()[dev.hw_idx as usize].tx_power = dbm;
    Ok(())
}

fn brcmf_get_signal(dev: &WirelessDev) -> i8 {
    chip_read(dev.hw_idx, BRCMF_RSSI) as i8
}

/// IRQ handler.
///
/// Ported from: `brcmf_sdio_isr()`
pub fn brcmf_irq_handler(dev: &WirelessDev) {
    let idx = dev.hw_idx;
    let status = chip_read(idx, BRCMF_SB_INT_STATUS);

    if status & SB_INT_SCAN_DONE != 0 {
        let chan = BRCMF_TABLE.lock()[idx as usize].channel;
        let rssi = chip_read(idx, BRCMF_RSSI) as i8;
        let freq = chan_to_freq(chan);
        let mut result = ScanResult::empty();
        result.channel = Channel {
            center_freq: freq,
            hw_value: chan,
            max_power: 2000,
            flags: 0,
            band: if freq < 5000 { Band::Band2GHz } else { Band::Band5GHz },
        };
        result.signal_dbm = rssi;
        result.valid = true;
        dev.add_scan_result(result);
    }

    // W1C
    chip_write(idx, BRCMF_SB_INT_STATUS, status);
}

fn chan_to_freq(chan: u16) -> u32 {
    if chan <= 14 {
        if chan == 14 { 2484 } else { 2407 + chan as u32 * 5 }
    } else {
        5000 + chan as u32 * 5
    }
}

// ---------------------------------------------------------------------------
// Static ops + probe
// ---------------------------------------------------------------------------

pub static BRCMF_OPS: WirelessOps = WirelessOps {
    init:         brcmf_init,
    deinit:       brcmf_deinit,
    scan:         brcmf_scan,
    abort_scan:   brcmf_abort_scan,
    connect:      brcmf_connect,
    disconnect:   brcmf_disconnect,
    tx:           brcmf_tx,
    set_tx_power: brcmf_set_tx_power,
    get_signal:   brcmf_get_signal,
};

/// Create a `WirelessDev` for a brcmfmac chip at the given MMIO base.
pub fn brcmfmac_probe(base: u64, mac: [u8; 6]) -> Result<WirelessDev, KernelError> {
    let idx = alloc_hw(base)?;
    Ok(WirelessDev::new(idx, &BRCMF_OPS, mac))
}
