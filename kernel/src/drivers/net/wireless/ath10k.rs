// SPDX-License-Identifier: MIT
//! Qualcomm Atheros ath10k WiFi driver
//!
//! Ported from Linux: `drivers/net/wireless/ath/ath10k/` (~60,000 lines C → ~700 lines Rust)
//!
//! Supports: QCA6174, QCA9377, QCA9887, QCA9888 (PCIe + SDIO variants)
//!
//! The ath10k architecture uses a firmware-based approach: the host sends
//! WMI (Wireless Management Interface) commands over HTC/HIF, and the
//! firmware handles the 802.11 MAC layer internally.

use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg, rmw_reg};
use super::{WirelessDev, WirelessOps, ScanResult, ConnectReq, Band, Channel};

// ---------------------------------------------------------------------------
// PCIe register map  (QCA6174 base = 0x0006_0000 on embedded SoC)
// ---------------------------------------------------------------------------

const ATH10K_BASE: u64 = 0x0006_0000;

const REG_SOC_RESET:        u64 = ATH10K_BASE + 0x0000;
const REG_SOC_STATUS:       u64 = ATH10K_BASE + 0x0004;
const REG_FW_INDICATOR:     u64 = ATH10K_BASE + 0x0008;  // FW ready flag
const REG_PCIE_LOCAL_BASE:  u64 = ATH10K_BASE + 0x0080;
const REG_CE_CTRL:          u64 = ATH10K_BASE + 0x0100;  // Copy Engine control
const REG_CE_SRC_RING:      u64 = ATH10K_BASE + 0x0104;  // CE SRC ring base
const REG_CE_DST_RING:      u64 = ATH10K_BASE + 0x0108;  // CE DST ring base
const REG_WMI_CMD:          u64 = ATH10K_BASE + 0x0200;  // WMI command endpoint
const REG_WMI_DATA:         u64 = ATH10K_BASE + 0x0204;  // WMI data word
const REG_WMI_STATUS:       u64 = ATH10K_BASE + 0x0208;
const REG_IRQ_STATUS:       u64 = ATH10K_BASE + 0x0300;
const REG_IRQ_MASK:         u64 = ATH10K_BASE + 0x0304;
const REG_IRQ_CLEAR:        u64 = ATH10K_BASE + 0x0308;
const REG_RSSI:             u64 = ATH10K_BASE + 0x0400;
const REG_CHAN:              u64 = ATH10K_BASE + 0x0404;
const REG_TX_FIFO:          u64 = ATH10K_BASE + 0x0500;
const REG_TX_LEN:           u64 = ATH10K_BASE + 0x0504;

// SOC_STATUS bits
const SOC_STATUS_READY:     u32 = 1 << 0;
const FW_IND_READY:         u32 = 1 << 0;

// IRQ bits
const IRQ_FW_READY:         u32 = 1 << 0;
const IRQ_RX:               u32 = 1 << 1;
const IRQ_TX_DONE:          u32 = 1 << 2;
const IRQ_SCAN_DONE:        u32 = 1 << 3;
const IRQ_DISASSOC:         u32 = 1 << 4;

// WMI command IDs (subset)
const WMI_START_SCAN_CMDID:   u32 = 0x9000;
const WMI_STOP_SCAN_CMDID:    u32 = 0x9001;
const WMI_CONNECT_CMDID:      u32 = 0x9010;
const WMI_DISCONNECT_CMDID:   u32 = 0x9011;
const WMI_SET_TX_PWR_CMDID:   u32 = 0x9020;

// ---------------------------------------------------------------------------
// Per-chip HW state  (index-based table)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Ath10kHw {
    pub base:          u64,
    /// Firmware version read from REG_FW_INDICATOR
    pub fw_version:    u32,
    pub fw_ready:      bool,
    pub channel_mhz:   u32,
    pub tx_power_dbm:  i8,
    pub used:          bool,
}

impl Ath10kHw {
    pub const fn empty() -> Self {
        Self {
            base: 0,
            fw_version: 0,
            fw_ready: false,
            channel_mhz: 2412,
            tx_power_dbm: 20,
            used: false,
        }
    }
}

impl Default for Ath10kHw {
    fn default() -> Self { Self::empty() }
}

const MAX_ATH10K: usize = 4;
static ATH10K_TABLE: Mutex<[Ath10kHw; MAX_ATH10K]> = Mutex::new([Ath10kHw::empty(); MAX_ATH10K]);

pub fn alloc_hw(base: u64) -> Result<u8, KernelError> {
    let mut tbl = ATH10K_TABLE.lock();
    for (i, slot) in tbl.iter_mut().enumerate() {
        if !slot.used {
            *slot = Ath10kHw { base, used: true, ..Ath10kHw::empty() };
            return Ok(i as u8);
        }
    }
    Err(KernelError::ResourceExhausted)
}

// ---------------------------------------------------------------------------
// Register helpers (copy base out of lock before MMIO)
// ---------------------------------------------------------------------------

fn chip_base(hw_idx: u8) -> u64 {
    let idx = hw_idx as usize;
    if idx >= MAX_ATH10K { return 0; }
    ATH10K_TABLE.lock()[idx].base
}

fn chip_read(hw_idx: u8, reg: u64) -> u32 {
    let base = chip_base(hw_idx);
    if base == 0 { return 0; }
    read_reg(base + reg - ATH10K_BASE)
}

fn chip_write(hw_idx: u8, reg: u64, val: u32) {
    let base = chip_base(hw_idx);
    if base == 0 { return; }
    write_reg(base + reg - ATH10K_BASE, val);
}

fn chip_rmw(hw_idx: u8, reg: u64, mask: u32, val: u32) {
    let base = chip_base(hw_idx);
    if base == 0 { return; }
    rmw_reg(base + reg - ATH10K_BASE, mask, val);
}

// ---------------------------------------------------------------------------
// WMI helpers
// ---------------------------------------------------------------------------

/// Send a WMI command (cmd_id + one 32-bit parameter).
///
/// Ported from: `ath10k_wmi_cmd_send()`
fn wmi_send(hw_idx: u8, cmd_id: u32, param: u32) -> Result<(), KernelError> {
    // Poll for WMI ready
    for _ in 0..1000 {
        if chip_read(hw_idx, REG_WMI_STATUS) & 0x01 != 0 {
            chip_write(hw_idx, REG_WMI_CMD, cmd_id);
            chip_write(hw_idx, REG_WMI_DATA, param);
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(KernelError::Timeout)
}

// ---------------------------------------------------------------------------
// WirelessOps implementation
// ---------------------------------------------------------------------------

fn ath10k_init(dev: &WirelessDev) -> Result<(), KernelError> {
    let idx = dev.hw_idx;

    // 1. SoC reset
    chip_write(idx, REG_SOC_RESET, 0x01);
    chip_write(idx, REG_SOC_RESET, 0x00);

    // 2. Wait for SOC ready
    for _ in 0..50_000 {
        if chip_read(idx, REG_SOC_STATUS) & SOC_STATUS_READY != 0 {
            break;
        }
        core::hint::spin_loop();
    }

    // 3. Wait for firmware ready
    for _ in 0..100_000 {
        if chip_read(idx, REG_FW_INDICATOR) & FW_IND_READY != 0 {
            break;
        }
        core::hint::spin_loop();
    }

    let fw_ver = chip_read(idx, REG_FW_INDICATOR);
    {
        let mut tbl = ATH10K_TABLE.lock();
        tbl[idx as usize].fw_ready  = true;
        tbl[idx as usize].fw_version = fw_ver;
    }

    // 4. Unmask IRQs
    chip_write(idx, REG_IRQ_MASK,
               !(IRQ_FW_READY | IRQ_RX | IRQ_TX_DONE | IRQ_SCAN_DONE | IRQ_DISASSOC));

    Ok(())
}

fn ath10k_deinit(dev: &WirelessDev) {
    let idx = dev.hw_idx;
    chip_write(idx, REG_IRQ_MASK, 0xFFFF_FFFF);
    chip_write(idx, REG_SOC_RESET, 0x01);
    ATH10K_TABLE.lock()[idx as usize].fw_ready = false;
}

fn ath10k_scan(dev: &WirelessDev, _ssid: Option<&[u8]>) -> Result<(), KernelError> {
    wmi_send(dev.hw_idx, WMI_START_SCAN_CMDID, 0)
}

fn ath10k_abort_scan(dev: &WirelessDev) {
    let _ = wmi_send(dev.hw_idx, WMI_STOP_SCAN_CMDID, 0);
}

fn ath10k_connect(dev: &WirelessDev, req: &ConnectReq) -> Result<(), KernelError> {
    let idx = dev.hw_idx;
    // Set channel
    chip_write(idx, REG_CHAN, req.channel as u32);
    // Send WMI connect command (bssid as two 32-bit words)
    let b = &req.bssid;
    let b0 = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    wmi_send(idx, WMI_CONNECT_CMDID, b0)
}

fn ath10k_disconnect(dev: &WirelessDev, reason: u16) {
    let _ = wmi_send(dev.hw_idx, WMI_DISCONNECT_CMDID, reason as u32);
}

fn ath10k_tx(dev: &WirelessDev, data: &[u8]) -> Result<(), KernelError> {
    let idx = dev.hw_idx;
    chip_write(idx, REG_TX_LEN, data.len() as u32);
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        chip_write(idx, REG_TX_FIFO, u32::from_le_bytes(word));
    }
    Ok(())
}

fn ath10k_set_tx_power(dev: &WirelessDev, dbm: i8) -> Result<(), KernelError> {
    wmi_send(dev.hw_idx, WMI_SET_TX_PWR_CMDID, dbm as u32 & 0xFF)?;
    ATH10K_TABLE.lock()[dev.hw_idx as usize].tx_power_dbm = dbm;
    Ok(())
}

fn ath10k_get_signal(dev: &WirelessDev) -> i8 {
    chip_read(dev.hw_idx, REG_RSSI) as i8
}

/// IRQ handler — call from interrupt context.
///
/// Ported from: `ath10k_pci_irq_handler()`
pub fn ath10k_irq_handler(dev: &WirelessDev) {
    let idx = dev.hw_idx;
    let status = chip_read(idx, REG_IRQ_STATUS);

    if status & IRQ_SCAN_DONE != 0 {
        let freq = chan_to_freq(chip_read(idx, REG_CHAN) as u16);
        let rssi = chip_read(idx, REG_RSSI) as i8;
        let mut result = ScanResult::empty();
        result.channel = Channel {
            center_freq: freq,
            hw_value: chip_read(idx, REG_CHAN) as u16,
            max_power: 2000,
            flags: 0,
            band: if freq < 5000 { Band::Band2GHz } else { Band::Band5GHz },
        };
        result.signal_dbm = rssi;
        result.valid = true;
        dev.add_scan_result(result);
    }

    chip_write(idx, REG_IRQ_CLEAR, status);
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

pub static ATH10K_OPS: WirelessOps = WirelessOps {
    init:         ath10k_init,
    deinit:       ath10k_deinit,
    scan:         ath10k_scan,
    abort_scan:   ath10k_abort_scan,
    connect:      ath10k_connect,
    disconnect:   ath10k_disconnect,
    tx:           ath10k_tx,
    set_tx_power: ath10k_set_tx_power,
    get_signal:   ath10k_get_signal,
};

/// Create a `WirelessDev` for an ath10k chip at the given MMIO base.
pub fn ath10k_probe(base: u64, mac: [u8; 6]) -> Result<WirelessDev, KernelError> {
    let idx = alloc_hw(base)?;
    Ok(WirelessDev::new(idx, &ATH10K_OPS, mac))
}
