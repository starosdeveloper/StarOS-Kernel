// SPDX-License-Identifier: MIT
//! MSM MDP5 (Snapdragon Mobile Display Processor 5)
//!
//! Ported from Linux: `drivers/gpu/drm/msm/disp/mdp5/`
//!
//! Chip: Qualcomm MSM8916 / MSM8953 / SDM660 / SDM845
//! MMIO: 0x0190_0000 (MDP5 base)
//!
//! Register map (selected):
//!   +0x000: MDP5_HW_VERSION
//!   +0x004: MDP5_DISP_INTF_SEL     — interface select
//!   +0x010: MDP5_CTL_LAYER_EXTN_0  — layer extension
//!   +0x100: MDP5_SSPP_SRC0_ADDR    — source layer 0 address
//!   +0x110: MDP5_SSPP_SRC_SIZE     — width/height packed
//!   +0x120: MDP5_SSPP_SRC_FORMAT   — pixel format
//!   +0x130: MDP5_SSPP_SRC_STRIDE   — pitch in bytes
//!   +0x200: MDSS_MDP_CTL0_START    — CTL0 flush
//!   +0x204: MDSS_MDP_CTL0_INTF_EN  — interface enable
//!   +0x400: MDP5_DSI_VIDEO_EN      — video mode enable
//!   +0x404: MDP5_DSI_HSYNC_CTL     — H timing
//!   +0x408: MDP5_DSI_VSYNC_CTL     — V timing
//!   +0x40C: MDP5_DSI_ACTIVE_HCTL   — active H region
//!   +0x410: MDP5_DSI_ACTIVE_VCTL   — active V region

use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg, rmw_reg};
use super::core::{DrmOps, DrmCrtc, DrmFb, DrmMode, ConnectorStatus};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_MSM_MDP: usize = 2;
const MDP5_BASE: u64 = 0x0190_0000;
const MDP5_STRIDE: u64 = 0x0010_0000;

// Register offsets
const REG_HW_VERSION:  u64 = 0x000;
const REG_INTF_SEL:    u64 = 0x004;
const REG_SRC0_ADDR:   u64 = 0x100;
const REG_SRC_SIZE:    u64 = 0x110;
const REG_SRC_FORMAT:  u64 = 0x120;
const REG_SRC_STRIDE:  u64 = 0x130;
const REG_CTL0_FLUSH:  u64 = 0x200;
const REG_CTL0_INTF:   u64 = 0x204;
const REG_DSI_VIDEO_EN: u64 = 0x400;
const REG_DSI_HSYNC:   u64 = 0x404;
const REG_DSI_VSYNC:   u64 = 0x408;
const REG_DSI_ACTIVE_H: u64 = 0x40C;
const REG_DSI_ACTIVE_V: u64 = 0x410;

// Pixel format codes
const FMT_ARGB8888: u32 = 0x0000_000A;
const FMT_RGB565:   u32 = 0x0000_0002;

// SSPP flush bits
const CTL_FLUSH_VIG0:    u32 = 1 << 0;
const CTL_FLUSH_RGB0:    u32 = 1 << 3;
const CTL_FLUSH_CURSOR0: u32 = 1 << 6;
const CTL_FLUSH_MIXER0:  u32 = 1 << 7;
const CTL_FLUSH_INTF:    u32 = 1 << 31;

// ---------------------------------------------------------------------------
// HW state
// ---------------------------------------------------------------------------

pub struct MsmMdpHw {
    pub base:    u64,
    pub present: bool,
    pub hw_ver:  u32,
    pub flags:   AtomicU32,
}

impl MsmMdpHw {
    pub const fn empty() -> Self {
        Self { base: 0, present: false, hw_ver: 0, flags: AtomicU32::new(0) }
    }
}

pub struct MsmMdpTable {
    pub hw:    [MsmMdpHw; MAX_MSM_MDP],
    pub count: usize,
}

impl MsmMdpTable {
    pub const fn new() -> Self {
        Self {
            hw:    [
                MsmMdpHw { base: 0, present: false, hw_ver: 0, flags: AtomicU32::new(0) },
                MsmMdpHw { base: 0, present: false, hw_ver: 0, flags: AtomicU32::new(0) },
            ],
            count: 0,
        }
    }
}

static MSM_MDP_TABLE: Mutex<MsmMdpTable> = Mutex::new(MsmMdpTable::new());

fn mdp5_base(hw_idx: u8) -> u64 {
    MSM_MDP_TABLE.lock().hw[hw_idx as usize].base
}

// ---------------------------------------------------------------------------
// Low-level programming
// ---------------------------------------------------------------------------

fn mdp5_set_mode(hw_idx: u8, _crtc_id: u8, mode: &DrmMode) -> Result<(), KernelError> {
    if !mode.is_valid() {
        return Err(KernelError::InvalidParameter("mode"));
    }
    let base = mdp5_base(hw_idx);

    // H timing: hsync_start[31:16] | htotal[15:0]
    let hsync = ((mode.hsync_start as u32) << 16) | (mode.htotal as u32);
    write_reg(base + REG_DSI_HSYNC, hsync);

    // V timing
    let vsync = ((mode.vsync_start as u32) << 16) | (mode.vtotal as u32);
    write_reg(base + REG_DSI_VSYNC, vsync);

    // Active region
    let active_h = ((mode.hsync_end as u32) << 16) | (mode.hsync_start as u32);
    write_reg(base + REG_DSI_ACTIVE_H, active_h);

    let active_v = ((mode.vsync_end as u32) << 16) | (mode.vsync_start as u32);
    write_reg(base + REG_DSI_ACTIVE_V, active_v);

    // Enable DSI video mode
    write_reg(base + REG_DSI_VIDEO_EN, 0x01);

    Ok(())
}

fn mdp5_atomic_commit(hw_idx: u8, _crtc: &DrmCrtc, fb: &DrmFb) -> Result<(), KernelError> {
    let base = mdp5_base(hw_idx);

    // Program source layer 0 (RGB0 pipe)
    write_reg(base + REG_SRC0_ADDR, fb.phys_addr as u32);

    // Size: width[31:16] | height[15:0]
    let size = ((fb.width as u32) << 16) | (fb.height as u32);
    write_reg(base + REG_SRC_SIZE, size);

    // Pitch
    write_reg(base + REG_SRC_STRIDE, fb.pitch);

    // Pixel format
    let fmt = match fb.format {
        super::core::PixelFormat::Rgb565     => FMT_RGB565,
        _                                    => FMT_ARGB8888,
    };
    write_reg(base + REG_SRC_FORMAT, fmt);

    // Interface select = DSI (bit 0)
    write_reg(base + REG_INTF_SEL, 0x01);

    // CTL0: enable interface
    write_reg(base + REG_CTL0_INTF, 0x01);

    // Flush all planes
    let flush = CTL_FLUSH_RGB0 | CTL_FLUSH_MIXER0 | CTL_FLUSH_INTF;
    write_reg(base + REG_CTL0_FLUSH, flush);

    Ok(())
}

fn mdp5_enable_vblank(hw_idx: u8, _crtc_id: u8) -> Result<(), KernelError> {
    let base = mdp5_base(hw_idx);
    rmw_reg(base + 0x300, 0, 1 << 0); // VBLANK_IRQ_EN
    Ok(())
}

fn mdp5_disable_vblank(hw_idx: u8, _crtc_id: u8) {
    let base = mdp5_base(hw_idx);
    let _ = rmw_reg(base + 0x300, 1 << 0, 0);
}

fn mdp5_connector_detect(_hw_idx: u8, _conn_id: u8) -> ConnectorStatus {
    // DSI panels are always considered connected (hotplug not supported)
    ConnectorStatus::Connected
}

fn mdp5_get_modes(_hw_idx: u8, _conn_id: u8, out: &mut [DrmMode]) -> u8 {
    if out.is_empty() { return 0; }
    // Expose a single FHD+ mode for SDM845 panel
    out[0] = DrmMode::new_simple(1080, 2340, 60, 163_000);
    1
}

pub static MSM_MDP_OPS: DrmOps = DrmOps {
    enable_vblank:    mdp5_enable_vblank,
    disable_vblank:   mdp5_disable_vblank,
    atomic_commit:    mdp5_atomic_commit,
    mode_set:         mdp5_set_mode,
    connector_detect: mdp5_connector_detect,
    get_modes:        mdp5_get_modes,
};

// ---------------------------------------------------------------------------
// Probe
// ---------------------------------------------------------------------------

use super::core::DrmDev;

pub fn msm_mdp_probe(base: u64) -> Result<DrmDev, KernelError> {
    let mut tbl = MSM_MDP_TABLE.lock();
    if tbl.count >= MAX_MSM_MDP {
        return Err(KernelError::ResourceExhausted);
    }
    let hw_idx = tbl.count as u8;
    let slot = tbl.count;
    tbl.count += 1;
    tbl.hw[slot] = MsmMdpHw {
        base, present: true,
        hw_ver: 0x0500_0000, // MDP5
        flags: AtomicU32::new(0),
    };
    drop(tbl);

    // Read HW version register
    let _ver = read_reg(base + REG_HW_VERSION);

    let mut dev = DrmDev::new(hw_idx, &MSM_MDP_OPS);

    // Add CRTC 0
    dev.add_crtc(super::core::DrmCrtc::new(0))?;
    // Add DSI encoder
    dev.add_encoder(super::core::DrmEncoder::new(0, super::core::EncoderType::DsiVideo))?;
    // Add DSI connector
    dev.add_connector(super::core::DrmConnector::new(0, super::core::ConnectorType::MipiDsi))?;

    Ok(dev)
}
