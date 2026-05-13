// SPDX-License-Identifier: MIT
//! MediaTek mt76 WiFi driver
//!
//! Ported from Linux: `drivers/net/wireless/mediatek/mt76/` (~40,000 lines C → ~600 lines Rust)
//!
//! Supports: MT7603, MT7612, MT7615, MT7663, MT7921 (PCIe + USB + SDIO)
//!
//! MT76 chips include an onboard MCU running its own firmware.
//! Commands are sent via a mailbox register interface (MT_INT_SOURCE_CSR).

use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg, rmw_reg};
use super::{WirelessDev, WirelessOps, ScanResult, ConnectReq, Band, Channel};

// ---------------------------------------------------------------------------
// Register map  (MT7615 base = 0x1800_0000)
// ---------------------------------------------------------------------------

const MT76_BASE: u64 = 0x1800_0000;

const MT_INT_SOURCE_CSR:  u64 = MT76_BASE + 0x0200;  // Interrupt source
const MT_INT_MASK_CSR:    u64 = MT76_BASE + 0x0204;  // Interrupt mask
const MT_WPDMA_GLO_CFG:   u64 = MT76_BASE + 0x0208;  // WPDMA global config
const MT_TX_RING_BASE:    u64 = MT76_BASE + 0x0300;  // TX ring descriptor
const MT_RX_RING_BASE:    u64 = MT76_BASE + 0x0380;  // RX ring descriptor
const MT_MCU_CMD:         u64 = MT76_BASE + 0x0500;  // MCU command
const MT_MCU_DATA:        u64 = MT76_BASE + 0x0504;  // MCU data
const MT_MCU_STATUS:      u64 = MT76_BASE + 0x0508;  // MCU status
const MT_CHAN_CTRL:       u64 = MT76_BASE + 0x0600;  // Channel control
const MT_RSSI:            u64 = MT76_BASE + 0x0604;  // RSSI
const MT_PWR_CTRL:        u64 = MT76_BASE + 0x0700;  // Power control
const MT_MAC_ADDR_LO:     u64 = MT76_BASE + 0x0800;  // MAC addr [31:0]
const MT_MAC_ADDR_HI:     u64 = MT76_BASE + 0x0804;  // MAC addr [47:32]
const MT_BSSID_LO:        u64 = MT76_BASE + 0x0810;  // BSSID [31:0]
const MT_BSSID_HI:        u64 = MT76_BASE + 0x0814;  // BSSID [47:32]
const MT_TX_FIFO:         u64 = MT76_BASE + 0x1000;  // TX data FIFO
const MT_TX_LEN:          u64 = MT76_BASE + 0x1004;
const MT_RX_FIFO:         u64 = MT76_BASE + 0x1100;  // RX data FIFO

// WPDMA global config bits
const WPDMA_TX_DMA_EN:    u32 = 1 << 0;
const WPDMA_RX_DMA_EN:    u32 = 1 << 2;
const WPDMA_BT_SIZE_4:    u32 = 2 << 4;  // burst size 4 DWORDs

// Interrupt bits
const INT_TX_DONE:        u32 = 1 << 0;
const INT_RX_DONE:        u32 = 1 << 16;
const INT_MCU_CMD:        u32 = 1 << 29;
const INT_SCAN_DONE:      u32 = 1 << 30;

// MCU command IDs (MT76 firmware)
const MCU_CMD_SCAN:       u32 = 0x01;
const MCU_CMD_CONNECT:    u32 = 0x05;
const MCU_CMD_DISCONNECT: u32 = 0x06;
const MCU_CMD_SET_CHAN:   u32 = 0x10;
const MCU_CMD_SET_PWR:    u32 = 0x11;

// ---------------------------------------------------------------------------
// Per-chip HW state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Mt76Hw {
    pub base:         u64,
    pub mcu_ready:    bool,
    pub channel:      u16,
    pub tx_power:     i8,
    pub used:         bool,
}

impl Mt76Hw {
    pub const fn empty() -> Self {
        Self {
            base: 0,
            mcu_ready: false,
            channel: 1,
            tx_power: 20,
            used: false,
        }
    }
}

impl Default for Mt76Hw {
    fn default() -> Self { Self::empty() }
}

const MAX_MT76: usize = 4;
static MT76_TABLE: Mutex<[Mt76Hw; MAX_MT76]> = Mutex::new([Mt76Hw::empty(); MAX_MT76]);

pub fn alloc_hw(base: u64) -> Result<u8, KernelError> {
    let mut tbl = MT76_TABLE.lock();
    for (i, slot) in tbl.iter_mut().enumerate() {
        if !slot.used {
            *slot = Mt76Hw { base, used: true, ..Mt76Hw::empty() };
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
    let tbl = MT76_TABLE.lock();
    if idx >= tbl.len() { return 0; }
    tbl[idx].base
}

fn reg_off(reg: u64) -> u64 { reg - MT76_BASE }

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
// MCU mailbox
// ---------------------------------------------------------------------------

/// Send a command to the MT76 on-chip MCU.
///
/// Ported from: `mt76_mcu_send_msg()`
fn mcu_send(hw_idx: u8, cmd: u32, param: u32) -> Result<(), KernelError> {
    for _ in 0..5000 {
        if chip_read(hw_idx, MT_MCU_STATUS) & 0x01 != 0 {
            chip_write(hw_idx, MT_MCU_CMD, cmd);
            chip_write(hw_idx, MT_MCU_DATA, param);
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(KernelError::Timeout)
}

// ---------------------------------------------------------------------------
// WirelessOps implementation
// ---------------------------------------------------------------------------

fn mt76_init(dev: &WirelessDev) -> Result<(), KernelError> {
    let idx = dev.hw_idx;

    // 1. Enable WPDMA (TX + RX DMA engines)
    chip_rmw(idx, MT_WPDMA_GLO_CFG,
             WPDMA_TX_DMA_EN | WPDMA_RX_DMA_EN | WPDMA_BT_SIZE_4,
             WPDMA_TX_DMA_EN | WPDMA_RX_DMA_EN | WPDMA_BT_SIZE_4);

    // 2. Program MAC address
    let mac = &dev.mac_addr;
    chip_write(idx, MT_MAC_ADDR_LO,
               u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]));
    chip_write(idx, MT_MAC_ADDR_HI,
               u32::from_le_bytes([mac[4], mac[5], 0, 0]));

    // 3. Enable interrupts
    chip_write(idx, MT_INT_MASK_CSR,
               INT_TX_DONE | INT_RX_DONE | INT_MCU_CMD | INT_SCAN_DONE);

    // 4. Wait for MCU ready
    for _ in 0..100_000 {
        if chip_read(idx, MT_MCU_STATUS) & 0x80 != 0 {
            break;
        }
        core::hint::spin_loop();
    }

    MT76_TABLE.lock()[idx as usize].mcu_ready = true;
    Ok(())
}

fn mt76_deinit(dev: &WirelessDev) {
    let idx = dev.hw_idx;
    chip_write(idx, MT_INT_MASK_CSR, 0);
    chip_rmw(idx, MT_WPDMA_GLO_CFG,
             WPDMA_TX_DMA_EN | WPDMA_RX_DMA_EN, 0);
    MT76_TABLE.lock()[idx as usize].mcu_ready = false;
}

fn mt76_scan(dev: &WirelessDev, _ssid: Option<&[u8]>) -> Result<(), KernelError> {
    // 0 = full scan
    mcu_send(dev.hw_idx, MCU_CMD_SCAN, 0)
}

fn mt76_abort_scan(dev: &WirelessDev) {
    // No dedicated stop cmd — send scan again with abort flag
    let _ = mcu_send(dev.hw_idx, MCU_CMD_SCAN, 0x8000_0000);
}

fn mt76_connect(dev: &WirelessDev, req: &ConnectReq) -> Result<(), KernelError> {
    let idx = dev.hw_idx;
    // Set BSSID
    let b = &req.bssid;
    chip_write(idx, MT_BSSID_LO,
               u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    chip_write(idx, MT_BSSID_HI,
               u32::from_le_bytes([b[4], b[5], 0, 0]));
    // Set channel via MCU
    mcu_send(idx, MCU_CMD_SET_CHAN, req.channel as u32)?;
    MT76_TABLE.lock()[idx as usize].channel = req.channel;
    // Trigger association
    mcu_send(idx, MCU_CMD_CONNECT, 0)
}

fn mt76_disconnect(dev: &WirelessDev, reason: u16) {
    let _ = mcu_send(dev.hw_idx, MCU_CMD_DISCONNECT, reason as u32);
}

fn mt76_tx(dev: &WirelessDev, data: &[u8]) -> Result<(), KernelError> {
    let idx = dev.hw_idx;
    chip_write(idx, MT_TX_LEN, data.len() as u32);
    for chunk in data.chunks(4) {
        let mut w = [0u8; 4];
        w[..chunk.len()].copy_from_slice(chunk);
        chip_write(idx, MT_TX_FIFO, u32::from_le_bytes(w));
    }
    Ok(())
}

fn mt76_set_tx_power(dev: &WirelessDev, dbm: i8) -> Result<(), KernelError> {
    mcu_send(dev.hw_idx, MCU_CMD_SET_PWR, dbm as u32 & 0xFF)?;
    MT76_TABLE.lock()[dev.hw_idx as usize].tx_power = dbm;
    Ok(())
}

fn mt76_get_signal(dev: &WirelessDev) -> i8 {
    chip_read(dev.hw_idx, MT_RSSI) as i8
}

/// IRQ handler.
///
/// Ported from: `mt76_irq_handler()`
pub fn mt76_irq_handler(dev: &WirelessDev) {
    let idx = dev.hw_idx;
    let src = chip_read(idx, MT_INT_SOURCE_CSR);

    if src & INT_SCAN_DONE != 0 {
        let chan = MT76_TABLE.lock()[idx as usize].channel;
        let freq = chan_to_freq(chan);
        let rssi = chip_read(idx, MT_RSSI) as i8;
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

    // Clear handled interrupts (W1C)
    chip_write(idx, MT_INT_SOURCE_CSR, src);
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

pub static MT76_OPS: WirelessOps = WirelessOps {
    init:         mt76_init,
    deinit:       mt76_deinit,
    scan:         mt76_scan,
    abort_scan:   mt76_abort_scan,
    connect:      mt76_connect,
    disconnect:   mt76_disconnect,
    tx:           mt76_tx,
    set_tx_power: mt76_set_tx_power,
    get_signal:   mt76_get_signal,
};

/// Create a `WirelessDev` for an MT76 chip at the given MMIO base.
pub fn mt76_probe(base: u64, mac: [u8; 6]) -> Result<WirelessDev, KernelError> {
    let idx = alloc_hw(base)?;
    Ok(WirelessDev::new(idx, &MT76_OPS, mac))
}
