// SPDX-License-Identifier: MIT
//! MIPI CSI-2 Receiver + Qualcomm CAMSS ISP
//!
//! Ported from Linux: `drivers/media/platform/qcom/camss/`
//!
//! Chip: Qualcomm SDM845 CAMSS (Camera Subsystem)
//! MMIO: CSIPHY 0x0AC6_5000, CSID 0x0AC6_6000, ISP 0x0ACE_0000
//!
//! Data flow:
//!   Sensor → CSIPHY (PHY) → CSID (decoder) → VFE (ISP) → output buffer
//!
//! Register map (CSID):
//!   +0x000: CSID_CORE_CTRL_0   — enable, version
//!   +0x004: CSID_CORE_CTRL_1   — vc/dt override
//!   +0x010: CSID_CID_N_CFG     — per-channel decode config
//!   +0x020: CSID_IRQ_CLEAR_CMD — IRQ clear
//!   +0x024: CSID_IRQ_MASK      — IRQ mask
//!   +0x028: CSID_IRQ_STATUS    — IRQ status

use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg};
use super::v4l2::V4l2PixFmt;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_CSI_DEVS: usize = 2;
pub const MAX_CSI_LANES: usize = 4;
pub const CSI_MAX_VC: usize = 4; // Virtual Channels

const CSIPHY_BASE: u64 = 0x0AC6_5000;
const CSID_BASE:   u64 = 0x0AC6_6000;
const VFE_BASE:    u64 = 0x0ACE_0000;
const CSI_STRIDE:  u64 = 0x0002_0000;

// Register offsets (CSID)
const REG_CSID_CTRL0:   u64 = 0x000;
const REG_CSID_CTRL1:   u64 = 0x004;
const REG_CSID_IRQ_CLR: u64 = 0x020;
const REG_CSID_IRQ_MASK: u64 = 0x024;
const REG_CSID_IRQ_ST:  u64 = 0x028;

// Register offsets (VFE)
const REG_VFE_CFG:      u64 = 0x000;
const REG_VFE_IRQ_CMD:  u64 = 0x058;
const REG_VFE_IRQ_MASK: u64 = 0x05C;
const REG_VFE_BUS_IMG0: u64 = 0x200; // output image buffer 0

// ---------------------------------------------------------------------------
// CSI-2 data types
// ---------------------------------------------------------------------------

pub mod csi_dt {
    pub const YUV422_8BIT:  u8 = 0x1E;
    pub const RAW8:         u8 = 0x2A;
    pub const RAW10:        u8 = 0x2B;
    pub const RAW12:        u8 = 0x2C;
    pub const RAW14:        u8 = 0x2D;
    pub const EMBEDDED_8BIT: u8 = 0x12;
}

// ---------------------------------------------------------------------------
// Sensor configuration
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct CsiSensorCfg {
    pub width:    u32,
    pub height:   u32,
    pub fmt:      V4l2PixFmt,
    pub data_type: u8,    // CSI-2 DT
    pub lanes:    u8,     // 1-4
    pub settle_cnt: u16,  // PHY settle count
    pub settle_clk: u8,   // PHY settle clock
}

impl CsiSensorCfg {
    pub const fn new_raw10_4lane(w: u32, h: u32) -> Self {
        Self {
            width: w, height: h,
            fmt: V4l2PixFmt::Raw10,
            data_type: csi_dt::RAW10,
            lanes: 4,
            settle_cnt: 0x1B,
            settle_clk: 0xFF,
        }
    }
}

// ---------------------------------------------------------------------------
// CsiOps vtable
// ---------------------------------------------------------------------------

pub struct CsiOps {
    pub phy_power_on:  fn(hw_idx: u8) -> Result<(), KernelError>,
    pub phy_power_off: fn(hw_idx: u8),
    pub csid_enable:   fn(hw_idx: u8, cfg: &CsiSensorCfg) -> Result<(), KernelError>,
    pub vfe_enable:    fn(hw_idx: u8, cfg: &CsiSensorCfg, buf_phys: u64) -> Result<(), KernelError>,
    pub vfe_disable:   fn(hw_idx: u8),
}

// ---------------------------------------------------------------------------
// CSI device state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsiState {
    Off,
    PhyOn,
    Streaming,
    Error,
}

pub struct CsiDev {
    pub hw_idx:  u8,
    pub ops:     &'static CsiOps,
    pub state:   CsiState,
    pub cfg:     CsiSensorCfg,
    pub frame_seq: AtomicU32,
}

impl CsiDev {
    pub const fn new(hw_idx: u8, ops: &'static CsiOps, cfg: CsiSensorCfg) -> Self {
        Self {
            hw_idx, ops,
            state: CsiState::Off,
            cfg,
            frame_seq: AtomicU32::new(0),
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

pub struct CsiDevTable {
    pub devs:  [Option<CsiDev>; MAX_CSI_DEVS],
    pub count: usize,
}

impl CsiDevTable {
    pub const fn new() -> Self {
        Self { devs: [None, None], count: 0 }
    }
}

pub static CSI_DEVS: Mutex<CsiDevTable> = Mutex::new(CsiDevTable::new());

// ---------------------------------------------------------------------------
// Low-level helpers
// ---------------------------------------------------------------------------

fn csi_phy_base(hw_idx: u8) -> u64 {
    CSIPHY_BASE + (hw_idx as u64) * CSI_STRIDE
}
fn csi_csid_base(hw_idx: u8) -> u64 {
    CSID_BASE + (hw_idx as u64) * CSI_STRIDE
}
fn csi_vfe_base(hw_idx: u8) -> u64 {
    VFE_BASE + (hw_idx as u64) * CSI_STRIDE
}

// ---------------------------------------------------------------------------
// CAMSS driver ops implementation (for Qualcomm SDM845)
// ---------------------------------------------------------------------------

fn camss_phy_power_on(hw_idx: u8) -> Result<(), KernelError> {
    let base = csi_phy_base(hw_idx);
    // Enable CSIPHY core clock, release reset
    write_reg(base + 0x000, 0x01); // CSIPHY_CTRL_0: enable
    write_reg(base + 0x004, 0x00); // CSIPHY_CTRL_1: release reset
    Ok(())
}

fn camss_phy_power_off(hw_idx: u8) {
    let base = csi_phy_base(hw_idx);
    write_reg(base + 0x000, 0x00);
}

fn camss_csid_enable(hw_idx: u8, cfg: &CsiSensorCfg) -> Result<(), KernelError> {
    let base = csi_csid_base(hw_idx);
    // Clear all IRQs
    write_reg(base + REG_CSID_IRQ_CLR, 0xFFFF_FFFF);
    // Enable core: bit0=enable, bits[5:4]=lane count
    let ctrl0 = 0x01 | ((cfg.lanes as u32 - 1) << 4);
    write_reg(base + REG_CSID_CTRL0, ctrl0);
    // Override data type
    write_reg(base + REG_CSID_CTRL1, cfg.data_type as u32);
    // Enable frame start/end IRQs
    write_reg(base + REG_CSID_IRQ_MASK, 0x07);
    Ok(())
}

fn camss_vfe_enable(hw_idx: u8, cfg: &CsiSensorCfg, buf_phys: u64) -> Result<(), KernelError> {
    let base = csi_vfe_base(hw_idx);
    // Configure VFE: input = CSID, output = RDI (raw dump)
    let vfe_cfg = (cfg.width << 16) | cfg.height;
    write_reg(base + REG_VFE_CFG, vfe_cfg);
    // Set output buffer
    write_reg(base + REG_VFE_BUS_IMG0, buf_phys as u32);
    // Enable IRQ for frame done (bit 0)
    write_reg(base + REG_VFE_IRQ_MASK, 0x01);
    // Clear IRQ
    write_reg(base + REG_VFE_IRQ_CMD, 0x01);
    Ok(())
}

fn camss_vfe_disable(hw_idx: u8) {
    let base = csi_vfe_base(hw_idx);
    write_reg(base + REG_VFE_CFG, 0x00);
    write_reg(base + REG_VFE_IRQ_MASK, 0x00);
}

pub static CAMSS_OPS: CsiOps = CsiOps {
    phy_power_on:  camss_phy_power_on,
    phy_power_off: camss_phy_power_off,
    csid_enable:   camss_csid_enable,
    vfe_enable:    camss_vfe_enable,
    vfe_disable:   camss_vfe_disable,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Probe a CSI device.
pub fn csi_probe(cfg: CsiSensorCfg) -> Result<u8, KernelError> {
    let mut tbl = CSI_DEVS.lock();
    if tbl.count >= MAX_CSI_DEVS {
        return Err(KernelError::ResourceExhausted);
    }
    let hw_idx = tbl.count as u8;
    let slot = tbl.count;
    tbl.count += 1;
    tbl.devs[slot] = Some(CsiDev::new(hw_idx, &CAMSS_OPS, cfg));
    Ok(slot as u8)
}

/// Start CSI streaming into a physical buffer.
pub fn csi_start(slot: u8, buf_phys: u64) -> Result<(), KernelError> {
    let (hw_idx, ops, cfg) = {
        let tbl = CSI_DEVS.lock();
        let d = tbl.devs[slot as usize].as_ref().ok_or(KernelError::NotFound)?;
        (d.hw_idx, d.ops, d.cfg)
    };
    (ops.phy_power_on)(hw_idx)?;
    (ops.csid_enable)(hw_idx, &cfg)?;
    (ops.vfe_enable)(hw_idx, &cfg, buf_phys)?;
    let mut tbl = CSI_DEVS.lock();
    if let Some(d) = &mut tbl.devs[slot as usize] {
        d.state = CsiState::Streaming;
    }
    Ok(())
}

/// Stop CSI streaming.
pub fn csi_stop(slot: u8) {
    let (hw_idx, ops) = {
        let tbl = CSI_DEVS.lock();
        if let Some(d) = tbl.devs[slot as usize].as_ref() {
            (d.hw_idx, d.ops)
        } else {
            return;
        }
    };
    (ops.vfe_disable)(hw_idx);
    (ops.phy_power_off)(hw_idx);
    let mut tbl = CSI_DEVS.lock();
    if let Some(d) = &mut tbl.devs[slot as usize] {
        d.state = CsiState::Off;
    }
}

/// ISP frame-done IRQ handler.
pub fn csi_vfe_irq_handler(slot: u8) -> u32 {
    let seq = {
        let tbl = CSI_DEVS.lock();
        if let Some(d) = tbl.devs[slot as usize].as_ref() {
            d.frame_seq.fetch_add(1, Ordering::Relaxed)
        } else {
            return 0;
        }
    };
    // Clear VFE IRQ (skip in test builds — hardware addresses not mapped)
    #[cfg(not(test))]
    {
        let hw_idx = CSI_DEVS.lock().devs[slot as usize].as_ref().map(|d| d.hw_idx).unwrap_or(0);
        let base = csi_vfe_base(hw_idx);
        write_reg(base + REG_VFE_IRQ_CMD, 0x01);
    }
    seq
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csi_sensor_cfg_raw10() {
        let cfg = CsiSensorCfg::new_raw10_4lane(4032, 3024);
        assert_eq!(cfg.lanes, 4);
        assert_eq!(cfg.data_type, csi_dt::RAW10);
        assert_eq!(cfg.width, 4032);
    }

    #[test]
    fn test_csi_probe() {
        let cfg = CsiSensorCfg::new_raw10_4lane(3264, 2448);
        let slot = csi_probe(cfg);
        assert!(slot.is_ok());
    }

    #[test]
    fn test_csi_initial_state_off() {
        // Check dev state directly
        let cfg = CsiSensorCfg::new_raw10_4lane(1920, 1080);
        let dev = CsiDev::new(99, &CAMSS_OPS, cfg);
        assert_eq!(dev.state, CsiState::Off);
    }

    #[test]
    fn test_csi_frame_seq_increments() {
        let cfg = CsiSensorCfg::new_raw10_4lane(640, 480);
        let slot = csi_probe(cfg).unwrap();
        let s0 = csi_vfe_irq_handler(slot);
        let s1 = csi_vfe_irq_handler(slot);
        assert_eq!(s1, s0 + 1);
    }

    #[test]
    fn test_csi_dt_constants() {
        assert_eq!(csi_dt::RAW10, 0x2B);
        assert_eq!(csi_dt::YUV422_8BIT, 0x1E);
    }
}
