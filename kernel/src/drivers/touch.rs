//! Touch screen driver for StarOS v0.3.0
//!
//! Supports:
//! - Goodix GT917S touchscreen
//! - Multi-touch (up to 5 points)
//! - I2C interface

use super::i2c::{I2cDriver, I2cError};

/// Touch event type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchEventType {
    Down,
    Move,
    Up,
}

/// Touch event
#[derive(Debug, Clone, Copy)]
pub struct TouchEvent {
    pub x: u16,
    pub y: u16,
    pub pressure: u8,
    pub event_type: TouchEventType,
    pub id: u8,
}

/// Goodix touchscreen driver
pub struct TouchDriver {
    i2c_bus: u8,
    i2c_addr: u8,
    width: u16,
    height: u16,
}

impl TouchDriver {
    /// Create new touch driver
    pub const fn new(i2c_bus: u8, i2c_addr: u8, width: u16, height: u16) -> Self {
        Self {
            i2c_bus,
            i2c_addr,
            width,
            height,
        }
    }

    /// Initialize touchscreen
    pub fn init(&mut self) -> Result<(), I2cError> {
        // Read product ID
        let mut buf = [0u8; 4];
        self.read_reg(0x8140, &mut buf)?;
        
        // Verify it's a Goodix chip
        if buf[0] != b'9' || buf[1] != b'1' || buf[2] != b'7' {
            return Err(I2cError::Nack);
        }

        // Configure touchscreen
        self.write_reg(0x8040, &[0x02])?; // Enable touch
        
        Ok(())
    }

    /// Poll for touch events
    pub fn poll(&mut self) -> Result<Option<TouchEvent>, I2cError> {
        // Read touch status
        let mut status = [0u8; 1];
        self.read_reg(0x814E, &mut status)?;
        
        let touch_count = status[0] & 0x0F;
        if touch_count == 0 {
            return Ok(None);
        }

        // Read first touch point
        let mut data = [0u8; 8];
        self.read_reg(0x8150, &mut data)?;
        
        let x = u16::from_le_bytes([data[0], data[1]]);
        let y = u16::from_le_bytes([data[2], data[3]]);
        let size = u16::from_le_bytes([data[4], data[5]]);
        
        // Clear touch status
        self.write_reg(0x814E, &[0])?;
        
        Ok(Some(TouchEvent {
            x: x.min(self.width - 1),
            y: y.min(self.height - 1),
            pressure: (size / 256) as u8,
            event_type: TouchEventType::Down,
            id: 0,
        }))
    }

    /// Read register
    fn read_reg(&mut self, reg: u16, buf: &mut [u8]) -> Result<(), I2cError> {
        let reg_bytes = reg.to_be_bytes();
        
        if let Some(i2c) = super::i2c::get_i2c(self.i2c_bus) {
            i2c.write(self.i2c_addr, &reg_bytes)?;
            i2c.read(self.i2c_addr, buf)?;
            Ok(())
        } else {
            Err(I2cError::Timeout)
        }
    }

    /// Write register
    fn write_reg(&mut self, reg: u16, data: &[u8]) -> Result<(), I2cError> {
        let reg_bytes = reg.to_be_bytes();
        let mut buf = [0u8; 10];
        buf[0] = reg_bytes[0];
        buf[1] = reg_bytes[1];
        buf[2..2 + data.len()].copy_from_slice(data);
        
        if let Some(i2c) = super::i2c::get_i2c(self.i2c_bus) {
            i2c.write(self.i2c_addr, &buf[..2 + data.len()])?;
            Ok(())
        } else {
            Err(I2cError::Timeout)
        }
    }
}

// Global touch driver
static mut TOUCH: Option<TouchDriver> = None;

/// Initialize touch driver
pub fn init_touch(i2c_bus: u8, i2c_addr: u8, width: u16, height: u16) {
    unsafe {
        let mut touch = TouchDriver::new(i2c_bus, i2c_addr, width, height);
        if touch.init().is_ok() {
            TOUCH = Some(touch);
        }
    }
}

/// Get touch driver
pub fn get_touch() -> Option<&'static mut TouchDriver> {
    unsafe { TOUCH.as_mut() }
}

/// Poll for touch event
pub fn poll_touch() -> Option<TouchEvent> {
    if let Some(touch) = get_touch() {
        touch.poll().ok().flatten()
    } else {
        None
    }
}
