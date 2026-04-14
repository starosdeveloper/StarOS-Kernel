// SPDX-License-Identifier: MIT
//! DRM Core + KMS Atomic Modesetting
//!
//! Ported from Linux: `drivers/gpu/drm/drm_crtc.c`, `drm_atomic.c`,
//!                    `drm_framebuffer.c`, `drm_modes.c`
//!
//! KMS pipeline (simplified):
//!
//! ┌────────────┐    ┌─────────────┐    ┌───────────────┐    ┌─────────────┐
//! │  Framebuffer│──▶│    CRTC     │──▶│    Encoder    │──▶│  Connector  │
//! │  (GPU mem) │    │(scan engine)│    │  (DSI/HDMI)   │    │(panel/HDMI) │
//! └────────────┘    └─────────────┘    └───────────────┘    └─────────────┘
//!
//! Atomic commit: all state changes applied at next vblank.

use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_DRM_DEVS:    usize = 2;
pub const MAX_CONNECTORS:  usize = 4;
pub const MAX_CRTCS:       usize = 4;
pub const MAX_ENCODERS:    usize = 4;
pub const MAX_FBS:         usize = 8;
pub const DRM_MODE_NAME_LEN: usize = 32;

// ---------------------------------------------------------------------------
// Display mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DrmMode {
    pub name:       [u8; DRM_MODE_NAME_LEN],
    pub name_len:   u8,
    pub clock_khz:  u32,  // pixel clock in kHz
    pub hdisplay:   u16,
    pub hsync_start: u16,
    pub hsync_end:  u16,
    pub htotal:     u16,
    pub vdisplay:   u16,
    pub vsync_start: u16,
    pub vsync_end:  u16,
    pub vtotal:     u16,
    pub flags:      u32,  // DRM_MODE_FLAG_*
    pub type_:      u8,   // DRM_MODE_TYPE_*
    pub vrefresh:   u8,   // Hz
}

impl DrmMode {
    pub const fn zero() -> Self {
        Self {
            name: [0u8; DRM_MODE_NAME_LEN],
            name_len: 0,
            clock_khz: 0,
            hdisplay: 0, hsync_start: 0, hsync_end: 0, htotal: 0,
            vdisplay: 0, vsync_start: 0, vsync_end: 0, vtotal: 0,
            flags: 0, type_: 0, vrefresh: 0,
        }
    }

    /// Construct a simple progressive mode.
    pub const fn new_simple(w: u16, h: u16, hz: u8, clock_khz: u32) -> Self {
        let mut m = Self::zero();
        m.hdisplay = w;
        m.vdisplay = h;
        m.vrefresh = hz;
        m.clock_khz = clock_khz;
        m.htotal = w + 160;
        m.vtotal = h + 45;
        m.hsync_start = w + 24;
        m.hsync_end   = w + 56;
        m.vsync_start = h + 5;
        m.vsync_end   = h + 14;
        m
    }

    pub fn is_valid(&self) -> bool {
        self.hdisplay > 0 && self.vdisplay > 0 && self.clock_khz > 0
    }
}

pub mod mode_flags {
    pub const NHSYNC:  u32 = 1 << 0;
    pub const PHSYNC:  u32 = 1 << 1;
    pub const NVSYNC:  u32 = 1 << 2;
    pub const PVSYNC:  u32 = 1 << 3;
    pub const INTERLACE: u32 = 1 << 4;
    pub const DBLSCAN:   u32 = 1 << 5;
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Xrgb8888,
    Argb8888,
    Rgb565,
    Yuyv,
}

#[derive(Clone, Copy)]
pub struct DrmFb {
    pub id:     u32,
    pub width:  u16,
    pub height: u16,
    pub pitch:  u32,   // bytes per row
    pub format: PixelFormat,
    pub phys_addr: u64, // physical (bus) address of pixel data
    pub size:   u32,   // total bytes
    pub valid:  bool,
}

impl DrmFb {
    pub const fn empty() -> Self {
        Self {
            id: 0, width: 0, height: 0, pitch: 0,
            format: PixelFormat::Xrgb8888,
            phys_addr: 0, size: 0, valid: false,
        }
    }
}

// ---------------------------------------------------------------------------
// CRTC
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrtcState {
    Disabled,
    Active,
}

#[derive(Clone, Copy)]
pub struct DrmCrtc {
    pub id:      u8,
    pub state:   CrtcState,
    pub mode:    DrmMode,
    pub fb_id:   u32,
    pub x:       u16, // viewport X offset
    pub y:       u16, // viewport Y offset
}

impl DrmCrtc {
    pub const fn new(id: u8) -> Self {
        Self {
            id, state: CrtcState::Disabled,
            mode: DrmMode::zero(), fb_id: 0, x: 0, y: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderType {
    None,
    Dac,
    Tmds,  // HDMI / DVI
    Lvds,
    DsiCmd,
    DsiVideo,
    Dp,
}

#[derive(Clone, Copy)]
pub struct DrmEncoder {
    pub id:       u8,
    pub enc_type: EncoderType,
    pub crtc_id:  u8,
}

impl DrmEncoder {
    pub const fn new(id: u8, enc_type: EncoderType) -> Self {
        Self { id, enc_type, crtc_id: 0 }
    }
}

// ---------------------------------------------------------------------------
// Connector
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorType {
    Unknown,
    VGA,
    DVI,
    HDMIA,
    HDMIB,
    MipiDsi,
    EDP,
    DP,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorStatus {
    Connected,
    Disconnected,
    Unknown,
}

pub const MAX_MODES_PER_CONNECTOR: usize = 8;

#[derive(Clone, Copy)]
pub struct DrmConnector {
    pub id:          u8,
    pub conn_type:   ConnectorType,
    pub status:      ConnectorStatus,
    pub encoder_id:  u8,
    pub modes:       [DrmMode; MAX_MODES_PER_CONNECTOR],
    pub mode_count:  u8,
    pub current_mode: DrmMode,
}

impl DrmConnector {
    pub const fn new(id: u8, conn_type: ConnectorType) -> Self {
        Self {
            id, conn_type,
            status: ConnectorStatus::Unknown,
            encoder_id: 0,
            modes: [DrmMode::zero(); MAX_MODES_PER_CONNECTOR],
            mode_count: 0,
            current_mode: DrmMode::zero(),
        }
    }

    pub fn add_mode(&mut self, mode: DrmMode) -> Result<(), KernelError> {
        if self.mode_count as usize >= MAX_MODES_PER_CONNECTOR {
            return Err(KernelError::ResourceExhausted);
        }
        self.modes[self.mode_count as usize] = mode;
        self.mode_count += 1;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DrmOps vtable
// ---------------------------------------------------------------------------

pub struct DrmOps {
    pub enable_vblank:  fn(hw_idx: u8, crtc_id: u8) -> Result<(), KernelError>,
    pub disable_vblank: fn(hw_idx: u8, crtc_id: u8),
    pub atomic_commit:  fn(hw_idx: u8, crtc: &DrmCrtc, fb: &DrmFb) -> Result<(), KernelError>,
    pub mode_set:       fn(hw_idx: u8, crtc_id: u8, mode: &DrmMode) -> Result<(), KernelError>,
    pub connector_detect: fn(hw_idx: u8, conn_id: u8) -> ConnectorStatus,
    pub get_modes:      fn(hw_idx: u8, conn_id: u8, out: &mut [DrmMode]) -> u8,
}

// ---------------------------------------------------------------------------
// DRM state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrmState {
    Uninitialized,
    Suspended,
    Active,
    Error,
}

// ---------------------------------------------------------------------------
// DRM device
// ---------------------------------------------------------------------------

pub struct DrmDev {
    pub hw_idx:     u8,
    pub ops:        &'static DrmOps,
    pub state:      DrmState,
    pub crtcs:      [DrmCrtc; MAX_CRTCS],
    pub crtc_count: u8,
    pub encoders:   [DrmEncoder; MAX_ENCODERS],
    pub enc_count:  u8,
    pub connectors: [DrmConnector; MAX_CONNECTORS],
    pub conn_count: u8,
    pub fbs:        [DrmFb; MAX_FBS],
    pub fb_count:   u8,
    pub next_fb_id: u32,
}

impl DrmDev {
    pub const fn new(hw_idx: u8, ops: &'static DrmOps) -> Self {
        Self {
            hw_idx,
            ops,
            state: DrmState::Uninitialized,
            crtcs:      [DrmCrtc::new(0); MAX_CRTCS],
            crtc_count: 0,
            encoders:   [DrmEncoder { id: 0, enc_type: EncoderType::None, crtc_id: 0 }; MAX_ENCODERS],
            enc_count:  0,
            connectors: [DrmConnector {
                id: 0, conn_type: ConnectorType::Unknown,
                status: ConnectorStatus::Unknown, encoder_id: 0,
                modes: [DrmMode::zero(); MAX_MODES_PER_CONNECTOR],
                mode_count: 0, current_mode: DrmMode::zero(),
            }; MAX_CONNECTORS],
            conn_count: 0,
            fbs:        [DrmFb::empty(); MAX_FBS],
            fb_count:   0,
            next_fb_id: 1,
        }
    }

    /// Add a CRTC to this device.
    pub fn add_crtc(&mut self, crtc: DrmCrtc) -> Result<(), KernelError> {
        if self.crtc_count as usize >= MAX_CRTCS {
            return Err(KernelError::ResourceExhausted);
        }
        self.crtcs[self.crtc_count as usize] = crtc;
        self.crtc_count += 1;
        Ok(())
    }

    /// Add an encoder.
    pub fn add_encoder(&mut self, enc: DrmEncoder) -> Result<(), KernelError> {
        if self.enc_count as usize >= MAX_ENCODERS {
            return Err(KernelError::ResourceExhausted);
        }
        self.encoders[self.enc_count as usize] = enc;
        self.enc_count += 1;
        Ok(())
    }

    /// Add a connector.
    pub fn add_connector(&mut self, conn: DrmConnector) -> Result<(), KernelError> {
        if self.conn_count as usize >= MAX_CONNECTORS {
            return Err(KernelError::ResourceExhausted);
        }
        self.connectors[self.conn_count as usize] = conn;
        self.conn_count += 1;
        Ok(())
    }

    /// Allocate a framebuffer.
    pub fn alloc_fb(&mut self, w: u16, h: u16, fmt: PixelFormat, phys: u64) -> Result<u32, KernelError> {
        if self.fb_count as usize >= MAX_FBS {
            return Err(KernelError::ResourceExhausted);
        }
        let bpp: u32 = match fmt {
            PixelFormat::Rgb565 => 2,
            PixelFormat::Yuyv   => 2,
            _                   => 4,
        };
        let pitch = w as u32 * bpp;
        let id = self.next_fb_id;
        self.next_fb_id += 1;
        let idx = self.fb_count as usize;
        self.fbs[idx] = DrmFb {
            id, width: w, height: h, pitch,
            format: fmt, phys_addr: phys,
            size: pitch * h as u32, valid: true,
        };
        self.fb_count += 1;
        Ok(id)
    }

    /// Find a framebuffer by ID.
    pub fn find_fb(&self, id: u32) -> Option<&DrmFb> {
        for i in 0..self.fb_count as usize {
            if self.fbs[i].id == id { return Some(&self.fbs[i]); }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Global registry
// ---------------------------------------------------------------------------

pub struct DrmDevTable {
    pub devs:  [Option<DrmDev>; MAX_DRM_DEVS],
    pub count: usize,
}

impl DrmDevTable {
    pub const fn new() -> Self {
        Self { devs: [None, None], count: 0 }
    }
}

pub static DRM_DEVS: Mutex<DrmDevTable> = Mutex::new(DrmDevTable::new());

/// Register a DRM device.
pub fn drm_register(dev: DrmDev) -> Result<u8, KernelError> {
    let mut tbl = DRM_DEVS.lock();
    if tbl.count >= MAX_DRM_DEVS {
        return Err(KernelError::ResourceExhausted);
    }
    let slot = tbl.count;
    tbl.devs[slot] = Some(dev);
    tbl.count += 1;
    Ok(slot as u8)
}

/// Atomic mode set: configure CRTC + connector, then commit.
pub fn drm_set_mode(slot: u8, crtc_id: u8, conn_id: u8, mode: DrmMode, fb_id: u32)
    -> Result<(), KernelError>
{
    let (hw_idx, ops) = {
        let tbl = DRM_DEVS.lock();
        let d = tbl.devs[slot as usize].as_ref().ok_or(KernelError::NotFound)?;
        (d.hw_idx, d.ops)
    };
    // Apply mode to CRTC via ops
    (ops.mode_set)(hw_idx, crtc_id, &mode)?;
    // Update CRTC state
    let mut tbl = DRM_DEVS.lock();
    if let Some(d) = &mut tbl.devs[slot as usize] {
        if crtc_id as usize >= d.crtc_count as usize {
            return Err(KernelError::InvalidParameter("crtc_id"));
        }
        d.crtcs[crtc_id as usize].mode = mode;
        d.crtcs[crtc_id as usize].fb_id = fb_id;
        d.crtcs[crtc_id as usize].state = CrtcState::Active;
        if (conn_id as usize) < (d.conn_count as usize) {
            d.connectors[conn_id as usize].current_mode = mode;
        }
    }
    Ok(())
}

/// Page flip: swap to a new framebuffer at the next vblank.
pub fn drm_flip(slot: u8, crtc_id: u8, fb_id: u32) -> Result<(), KernelError> {
    let (hw_idx, ops, crtc, fb) = {
        let tbl = DRM_DEVS.lock();
        let d = tbl.devs[slot as usize].as_ref().ok_or(KernelError::NotFound)?;
        if crtc_id as usize >= d.crtc_count as usize {
            return Err(KernelError::InvalidParameter("crtc_id"));
        }
        let fb = d.find_fb(fb_id).ok_or(KernelError::NotFound)?.clone();
        (d.hw_idx, d.ops, d.crtcs[crtc_id as usize], fb)
    };
    (ops.atomic_commit)(hw_idx, &crtc, &fb)?;
    let mut tbl = DRM_DEVS.lock();
    if let Some(d) = &mut tbl.devs[slot as usize] {
        d.crtcs[crtc_id as usize].fb_id = fb_id;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    static DUMMY_OPS: DrmOps = DrmOps {
        enable_vblank:    |_, _| Ok(()),
        disable_vblank:   |_, _| {},
        atomic_commit:    |_, _, _| Ok(()),
        mode_set:         |_, _, _| Ok(()),
        connector_detect: |_, _| ConnectorStatus::Connected,
        get_modes:        |_, _, _| 0,
    };

    fn make_dev() -> DrmDev {
        DrmDev::new(0, &DUMMY_OPS)
    }

    #[test]
    fn test_drm_mode_valid() {
        let m = DrmMode::new_simple(1920, 1080, 60, 148_500);
        assert!(m.is_valid());
        assert_eq!(m.hdisplay, 1920);
        assert_eq!(m.vdisplay, 1080);
        assert_eq!(m.vrefresh, 60);
    }

    #[test]
    fn test_drm_mode_zero_invalid() {
        assert!(!DrmMode::zero().is_valid());
    }

    #[test]
    fn test_drm_fb_alloc() {
        let mut dev = make_dev();
        let id = dev.alloc_fb(1920, 1080, PixelFormat::Xrgb8888, 0x8000_0000).unwrap();
        assert_eq!(id, 1);
        let fb = dev.find_fb(id).unwrap();
        assert!(fb.valid);
        assert_eq!(fb.width, 1920);
        assert_eq!(fb.height, 1080);
        assert_eq!(fb.pitch, 1920 * 4);
    }

    #[test]
    fn test_drm_fb_not_found() {
        let dev = make_dev();
        assert!(dev.find_fb(999).is_none());
    }

    #[test]
    fn test_drm_add_crtc_encoder_connector() {
        let mut dev = make_dev();
        dev.add_crtc(DrmCrtc::new(0)).unwrap();
        dev.add_encoder(DrmEncoder::new(0, EncoderType::DsiCmd)).unwrap();
        let mut conn = DrmConnector::new(0, ConnectorType::MipiDsi);
        let mode = DrmMode::new_simple(1080, 2340, 60, 163_000);
        conn.add_mode(mode).unwrap();
        dev.add_connector(conn).unwrap();
        assert_eq!(dev.crtc_count, 1);
        assert_eq!(dev.enc_count, 1);
        assert_eq!(dev.conn_count, 1);
        assert_eq!(dev.connectors[0].mode_count, 1);
    }

    #[test]
    fn test_drm_register_and_set_mode() {
        // Use a fresh DrmDev (note: shares global DRM_DEVS with other tests)
        let mut dev = DrmDev::new(55, &DUMMY_OPS);
        dev.add_crtc(DrmCrtc::new(0)).unwrap();
        let mut conn = DrmConnector::new(0, ConnectorType::MipiDsi);
        conn.status = ConnectorStatus::Connected;
        dev.add_connector(conn).unwrap();
        let fb_id = dev.alloc_fb(1080, 2340, PixelFormat::Xrgb8888, 0x8800_0000).unwrap();

        let slot = {
            let mut tbl = DRM_DEVS.lock();
            let s = tbl.count;
            if s < MAX_DRM_DEVS {
                tbl.devs[s] = Some(dev);
                tbl.count += 1;
            }
            s as u8
        };

        let mode = DrmMode::new_simple(1080, 2340, 60, 163_000);
        assert!(drm_set_mode(slot, 0, 0, mode, fb_id).is_ok());
    }

    #[test]
    fn test_crtc_state_after_mode_set() {
        let mut dev = make_dev();
        dev.add_crtc(DrmCrtc::new(0)).unwrap();
        assert_eq!(dev.crtcs[0].state, CrtcState::Disabled);
        // Set mode directly
        dev.crtcs[0].mode = DrmMode::new_simple(720, 1280, 60, 75_000);
        dev.crtcs[0].state = CrtcState::Active;
        assert_eq!(dev.crtcs[0].state, CrtcState::Active);
        assert_eq!(dev.crtcs[0].mode.hdisplay, 720);
    }

    #[test]
    fn test_connector_modes() {
        let mut conn = DrmConnector::new(0, ConnectorType::HDMIA);
        let m1080 = DrmMode::new_simple(1920, 1080, 60, 148_500);
        let m720  = DrmMode::new_simple(1280, 720,  60,  74_250);
        conn.add_mode(m1080).unwrap();
        conn.add_mode(m720).unwrap();
        assert_eq!(conn.mode_count, 2);
        assert_eq!(conn.modes[0].hdisplay, 1920);
        assert_eq!(conn.modes[1].hdisplay, 1280);
    }
}
