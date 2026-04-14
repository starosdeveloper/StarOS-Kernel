// SPDX-License-Identifier: MIT
//! MIPI DSI Protocol + Panel Driver Interface
//!
//! Ported from Linux: `drivers/gpu/drm/drm_mipi_dsi.c`,
//!                    `include/drm/drm_mipi_dsi.h`
//!
//! MIPI DSI frame types (data type):
//!   0x05: Short write, no parameter
//!   0x06: Read, no parameter
//!   0x07: End of transmission packet
//!   0x15: Short write, 1 parameter
//!   0x37: Set maximum return packet size
//!   0x39: Long write (generic)

use spin::Mutex;
use crate::error::KernelError;
use crate::drivers::clk::mmio::{read_reg, write_reg};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_MIPI_DEVS:    usize = 2;
pub const MIPI_MSG_MAX_LEN: usize = 256;

// Data type codes (MIPI DSI spec Table 8-1)
pub mod dt {
    pub const VSYNC_START:     u8 = 0x01;
    pub const VSYNC_END:       u8 = 0x11;
    pub const HSYNC_START:     u8 = 0x21;
    pub const HSYNC_END:       u8 = 0x31;
    pub const EOT_PACKET:      u8 = 0x08;
    pub const DCS_SHORT_W_0P:  u8 = 0x05;
    pub const DCS_SHORT_W_1P:  u8 = 0x15;
    pub const DCS_LONG_W:      u8 = 0x39;
    pub const DCS_READ_0P:     u8 = 0x06;
    pub const SET_MAX_RTN:     u8 = 0x37;
    pub const GENERIC_SHORT_W0:u8 = 0x03;
    pub const GENERIC_LONG_W:  u8 = 0x29;
}

// DCS (Display Command Set) commands
pub mod dcs {
    pub const NOP:                u8 = 0x00;
    pub const SOFT_RESET:         u8 = 0x01;
    pub const GET_POWER_MODE:     u8 = 0x0A;
    pub const GET_ADDRESS_MODE:   u8 = 0x0B;
    pub const ENTER_SLEEP_MODE:   u8 = 0x10;
    pub const EXIT_SLEEP_MODE:    u8 = 0x11;
    pub const ENTER_PARTIAL_MODE: u8 = 0x12;
    pub const ENTER_NORMAL_MODE:  u8 = 0x13;
    pub const SET_DISPLAY_OFF:    u8 = 0x28;
    pub const SET_DISPLAY_ON:     u8 = 0x29;
    pub const SET_COLUMN_ADDRESS: u8 = 0x2A;
    pub const SET_PAGE_ADDRESS:   u8 = 0x2B;
    pub const WRITE_MEMORY_START: u8 = 0x2C;
    pub const SET_PIXEL_FORMAT:   u8 = 0x3A;
    pub const SET_SCROLL_START:   u8 = 0x37;
    pub const WRITE_CTRL_DISPLAY: u8 = 0x53;
    pub const SET_BRIGHTNESS:     u8 = 0x51;
}

// ---------------------------------------------------------------------------
// MIPI DSI message
// ---------------------------------------------------------------------------

pub struct MipiDsiMsg {
    pub channel:  u8,
    pub data_type: u8,
    pub flags:    u16,
    pub tx_buf:   [u8; MIPI_MSG_MAX_LEN],
    pub tx_len:   usize,
    pub rx_buf:   [u8; 64],
    pub rx_len:   usize,
}

impl MipiDsiMsg {
    pub const fn new_dcs_write(channel: u8, data: &'static [u8]) -> Self {
        // can't do loops in const fn, use a basic approach
        let data_type = if data.len() == 1 { dt::DCS_SHORT_W_0P }
                        else if data.len() == 2 { dt::DCS_SHORT_W_1P }
                        else { dt::DCS_LONG_W };
        let _ = data_type; // avoid unused warning in const context
        Self {
            channel,
            data_type: dt::DCS_LONG_W, // simplify for const context
            flags: 0,
            tx_buf: [0u8; MIPI_MSG_MAX_LEN],
            tx_len: 0,
            rx_buf: [0u8; 64],
            rx_len: 0,
        }
    }

    pub fn dcs_write(channel: u8, data: &[u8]) -> Result<Self, KernelError> {
        if data.is_empty() || data.len() > MIPI_MSG_MAX_LEN {
            return Err(KernelError::InvalidParameter(""));
        }
        let data_type = if data.len() == 1 { dt::DCS_SHORT_W_0P }
                        else if data.len() == 2 { dt::DCS_SHORT_W_1P }
                        else { dt::DCS_LONG_W };
        let mut msg = Self {
            channel, data_type, flags: 0,
            tx_buf: [0u8; MIPI_MSG_MAX_LEN],
            tx_len: data.len(),
            rx_buf: [0u8; 64],
            rx_len: 0,
        };
        msg.tx_buf[..data.len()].copy_from_slice(data);
        Ok(msg)
    }

    pub fn dcs_read(channel: u8, cmd: u8, rx_len: usize) -> Self {
        let mut msg = Self {
            channel,
            data_type: dt::DCS_READ_0P,
            flags: 0,
            tx_buf: [0u8; MIPI_MSG_MAX_LEN],
            tx_len: 1,
            rx_buf: [0u8; 64],
            rx_len: rx_len.min(64),
        };
        msg.tx_buf[0] = cmd;
        msg
    }

    pub fn tx_data(&self) -> &[u8] {
        &self.tx_buf[..self.tx_len]
    }
    pub fn rx_data(&self) -> &[u8] {
        &self.rx_buf[..self.rx_len]
    }
}

// ---------------------------------------------------------------------------
// MipiDsiOps vtable
// ---------------------------------------------------------------------------

pub struct MipiDsiOps {
    pub transfer:    fn(hw_idx: u8, msg: &mut MipiDsiMsg) -> Result<usize, KernelError>,
    pub power_on:    fn(hw_idx: u8) -> Result<(), KernelError>,
    pub power_off:   fn(hw_idx: u8),
    pub set_lanes:   fn(hw_idx: u8, lanes: u8),
    pub set_format:  fn(hw_idx: u8, format: DsiPixelFmt),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DsiPixelFmt {
    Rgb888,
    Rgb666,
    Rgb565,
}

// ---------------------------------------------------------------------------
// Backlight control
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct BacklightState {
    pub brightness: u8,   // 0-255
    pub enabled:    bool,
}

impl BacklightState {
    pub const fn off() -> Self { Self { brightness: 0, enabled: false } }
}

// ---------------------------------------------------------------------------
// Panel state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelState {
    Off,
    Initialized,
    On,
    Sleeping,
}

// ---------------------------------------------------------------------------
// MIPI DSI device
// ---------------------------------------------------------------------------

pub struct MipiDsiDev {
    pub hw_idx:     u8,
    pub ops:        &'static MipiDsiOps,
    pub state:      PanelState,
    pub lanes:      u8,
    pub format:     DsiPixelFmt,
    pub channel:    u8,
    pub backlight:  BacklightState,
}

impl MipiDsiDev {
    pub const fn new(hw_idx: u8, ops: &'static MipiDsiOps, lanes: u8) -> Self {
        Self {
            hw_idx,
            ops,
            state: PanelState::Off,
            lanes,
            format: DsiPixelFmt::Rgb888,
            channel: 0,
            backlight: BacklightState::off(),
        }
    }

    /// Initialize the panel: power on, set lanes, send init sequence.
    pub fn panel_init(&mut self) -> Result<(), KernelError> {
        (self.ops.power_on)(self.hw_idx)?;
        (self.ops.set_lanes)(self.hw_idx, self.lanes);
        (self.ops.set_format)(self.hw_idx, self.format);

        // Standard DCS init sequence
        self.dcs_short_write(dcs::SOFT_RESET)?;
        self.dcs_short_write(dcs::EXIT_SLEEP_MODE)?;
        self.state = PanelState::Initialized;
        Ok(())
    }

    /// Turn the display on.
    pub fn display_on(&mut self) -> Result<(), KernelError> {
        self.dcs_short_write(dcs::SET_DISPLAY_ON)?;
        self.state = PanelState::On;
        Ok(())
    }

    /// Turn the display off.
    pub fn display_off(&mut self) -> Result<(), KernelError> {
        self.dcs_short_write(dcs::SET_DISPLAY_OFF)?;
        self.state = PanelState::Initialized;
        Ok(())
    }

    /// Set backlight brightness (0-255).
    pub fn set_brightness(&mut self, level: u8) -> Result<(), KernelError> {
        let mut msg = MipiDsiMsg::dcs_write(self.channel, &[dcs::SET_BRIGHTNESS, level])?;
        let hw = self.hw_idx;
        (self.ops.transfer)(hw, &mut msg)?;
        self.backlight.brightness = level;
        self.backlight.enabled = level > 0;
        Ok(())
    }

    /// Set pixel format via DCS.
    pub fn set_pixel_format(&mut self, fmt_byte: u8) -> Result<(), KernelError> {
        self.dcs_short_write_param(dcs::SET_PIXEL_FORMAT, fmt_byte)
    }

    /// Set column (x) address range.
    pub fn set_column_address(&mut self, x_start: u16, x_end: u16) -> Result<(), KernelError> {
        let data = [
            dcs::SET_COLUMN_ADDRESS,
            (x_start >> 8) as u8, x_start as u8,
            (x_end >> 8) as u8,   x_end as u8,
        ];
        let mut msg = MipiDsiMsg::dcs_write(self.channel, &data)?;
        let hw = self.hw_idx;
        (self.ops.transfer)(hw, &mut msg)?;
        Ok(())
    }

    /// Set page (y) address range.
    pub fn set_page_address(&mut self, y_start: u16, y_end: u16) -> Result<(), KernelError> {
        let data = [
            dcs::SET_PAGE_ADDRESS,
            (y_start >> 8) as u8, y_start as u8,
            (y_end >> 8) as u8,   y_end as u8,
        ];
        let mut msg = MipiDsiMsg::dcs_write(self.channel, &data)?;
        let hw = self.hw_idx;
        (self.ops.transfer)(hw, &mut msg)?;
        Ok(())
    }

    fn dcs_short_write(&mut self, cmd: u8) -> Result<(), KernelError> {
        let mut msg = MipiDsiMsg::dcs_write(self.channel, &[cmd])?;
        let hw = self.hw_idx;
        (self.ops.transfer)(hw, &mut msg)?;
        Ok(())
    }

    fn dcs_short_write_param(&mut self, cmd: u8, param: u8) -> Result<(), KernelError> {
        let mut msg = MipiDsiMsg::dcs_write(self.channel, &[cmd, param])?;
        let hw = self.hw_idx;
        (self.ops.transfer)(hw, &mut msg)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Global MIPI DSI device table
// ---------------------------------------------------------------------------

pub struct MipiDevTable {
    pub devs:  [Option<MipiDsiDev>; MAX_MIPI_DEVS],
    pub count: usize,
}

impl MipiDevTable {
    pub const fn new() -> Self {
        Self { devs: [None, None], count: 0 }
    }
}

pub static MIPI_DEVS: Mutex<MipiDevTable> = Mutex::new(MipiDevTable::new());

/// Probe and register a MIPI DSI device.
pub fn mipi_dsi_probe(dev: MipiDsiDev) -> Result<u8, KernelError> {
    let mut tbl = MIPI_DEVS.lock();
    if tbl.count >= MAX_MIPI_DEVS {
        return Err(KernelError::ResourceExhausted);
    }
    let slot = tbl.count;
    tbl.devs[slot] = Some(dev);
    tbl.count += 1;
    Ok(slot as u8)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    static DUMMY_OPS: MipiDsiOps = MipiDsiOps {
        transfer:  |_, msg| { Ok(msg.tx_len) },
        power_on:  |_| Ok(()),
        power_off: |_| {},
        set_lanes: |_, _| {},
        set_format:|_, _| {},
    };

    #[test]
    fn test_mipi_msg_dcs_short_write() {
        let msg = MipiDsiMsg::dcs_write(0, &[dcs::EXIT_SLEEP_MODE]).unwrap();
        assert_eq!(msg.data_type, dt::DCS_SHORT_W_0P);
        assert_eq!(msg.tx_len, 1);
        assert_eq!(msg.tx_data()[0], dcs::EXIT_SLEEP_MODE);
    }

    #[test]
    fn test_mipi_msg_dcs_long_write() {
        let data = [dcs::SET_COLUMN_ADDRESS, 0x00, 0x00, 0x04, 0x37];
        let msg = MipiDsiMsg::dcs_write(0, &data).unwrap();
        assert_eq!(msg.data_type, dt::DCS_LONG_W);
        assert_eq!(msg.tx_len, 5);
    }

    #[test]
    fn test_mipi_msg_empty_fails() {
        assert!(MipiDsiMsg::dcs_write(0, &[]).is_err());
    }

    #[test]
    fn test_panel_init() {
        let mut dev = MipiDsiDev::new(0, &DUMMY_OPS, 4);
        assert_eq!(dev.state, PanelState::Off);
        dev.panel_init().unwrap();
        assert_eq!(dev.state, PanelState::Initialized);
    }

    #[test]
    fn test_display_on_off() {
        let mut dev = MipiDsiDev::new(0, &DUMMY_OPS, 4);
        dev.panel_init().unwrap();
        dev.display_on().unwrap();
        assert_eq!(dev.state, PanelState::On);
        dev.display_off().unwrap();
        assert_eq!(dev.state, PanelState::Initialized);
    }

    #[test]
    fn test_set_brightness() {
        let mut dev = MipiDsiDev::new(0, &DUMMY_OPS, 4);
        dev.set_brightness(200).unwrap();
        assert_eq!(dev.backlight.brightness, 200);
        assert!(dev.backlight.enabled);
        dev.set_brightness(0).unwrap();
        assert!(!dev.backlight.enabled);
    }

    #[test]
    fn test_set_column_address() {
        let mut dev = MipiDsiDev::new(0, &DUMMY_OPS, 4);
        assert!(dev.set_column_address(0, 1079).is_ok());
    }

    #[test]
    fn test_mipi_dsi_probe_registration() {
        let dev = MipiDsiDev::new(88, &DUMMY_OPS, 2);
        let slot = mipi_dsi_probe(dev);
        assert!(slot.is_ok());
    }
}
