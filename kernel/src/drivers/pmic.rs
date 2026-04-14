//! Power Management IC driver for StarOS v0.3.0
//!
//! Supports:
//! - Rockchip RK818 PMIC
//! - Battery monitoring
//! - Voltage regulation
//! - Power control

use super::i2c::{I2cDriver, I2cError};

/// RK818 PMIC registers
const REG_SECONDS: u8 = 0x00;
const REG_INT_STS1: u8 = 0x4C;
const REG_INT_STS2: u8 = 0x4D;
const REG_VB_MON: u8 = 0xA0;
const REG_SUP_STS: u8 = 0xA0;
const REG_USB_CTRL: u8 = 0xA1;
const REG_CHRG_CTRL1: u8 = 0x99;
const REG_DCDC_EN: u8 = 0x23;
const REG_LDO_EN: u8 = 0x27;

/// PMIC driver
pub struct PmicDriver {
    i2c_bus: u8,
    i2c_addr: u8,
}

impl PmicDriver {
    /// Create new PMIC driver
    pub const fn new(i2c_bus: u8, i2c_addr: u8) -> Self {
        Self { i2c_bus, i2c_addr }
    }

    /// Initialize PMIC
    pub fn init(&mut self) -> Result<(), I2cError> {
        // Read chip ID to verify communication
        let mut id = [0u8; 1];
        self.read_reg(0x17, &mut id)?;
        
        // Enable all DC-DC converters
        self.write_reg(REG_DCDC_EN, &[0x0F])?;
        
        // Enable all LDOs
        self.write_reg(REG_LDO_EN, &[0xFF])?;
        
        Ok(())
    }

    /// Read battery voltage (in millivolts)
    pub fn read_battery_voltage(&mut self) -> Result<u16, I2cError> {
        let mut data = [0u8; 2];
        self.read_reg(REG_VB_MON, &mut data)?;
        
        // Convert ADC value to millivolts
        // RK818: 12-bit ADC, 1.8V reference, 4x divider
        let adc = u16::from_le_bytes(data) & 0x0FFF;
        let voltage = (adc as u32 * 1800 * 4 / 4096) as u16;
        
        Ok(voltage)
    }

    /// Read battery level (0-100%)
    pub fn read_battery_level(&mut self) -> Result<u8, I2cError> {
        let voltage = self.read_battery_voltage()?;
        
        // Simple voltage-to-percentage mapping
        // 3.0V = 0%, 4.2V = 100%
        let level = if voltage < 3000 {
            0
        } else if voltage > 4200 {
            100
        } else {
            ((voltage - 3000) * 100 / 1200) as u8
        };
        
        Ok(level)
    }

    /// Check if USB is connected
    pub fn is_usb_connected(&mut self) -> Result<bool, I2cError> {
        let mut status = [0u8; 1];
        self.read_reg(REG_SUP_STS, &mut status)?;
        Ok(status[0] & 0x02 != 0)
    }

    /// Check if charging
    pub fn is_charging(&mut self) -> Result<bool, I2cError> {
        let mut status = [0u8; 1];
        self.read_reg(REG_SUP_STS, &mut status)?;
        Ok(status[0] & 0x01 != 0)
    }

    /// Set CPU voltage (in millivolts)
    pub fn set_cpu_voltage(&mut self, voltage_mv: u16) -> Result<(), I2cError> {
        // DCDC1 controls CPU voltage
        // Formula: Vout = 0.7125V + (REG * 12.5mV)
        let reg_val = if voltage_mv < 712 {
            0
        } else {
            ((voltage_mv - 712) / 12).min(63)
        };
        
        self.write_reg(0x2F, &[reg_val as u8])?;
        Ok(())
    }

    /// Enable regulator
    pub fn enable_regulator(&mut self, id: u8) -> Result<(), I2cError> {
        if id < 4 {
            // DC-DC converter
            let mut en = [0u8; 1];
            self.read_reg(REG_DCDC_EN, &mut en)?;
            en[0] |= 1 << id;
            self.write_reg(REG_DCDC_EN, &en)?;
        } else if id < 12 {
            // LDO
            let mut en = [0u8; 1];
            self.read_reg(REG_LDO_EN, &mut en)?;
            en[0] |= 1 << (id - 4);
            self.write_reg(REG_LDO_EN, &en)?;
        }
        Ok(())
    }

    /// Disable regulator
    pub fn disable_regulator(&mut self, id: u8) -> Result<(), I2cError> {
        if id < 4 {
            // DC-DC converter
            let mut en = [0u8; 1];
            self.read_reg(REG_DCDC_EN, &mut en)?;
            en[0] &= !(1 << id);
            self.write_reg(REG_DCDC_EN, &en)?;
        } else if id < 12 {
            // LDO
            let mut en = [0u8; 1];
            self.read_reg(REG_LDO_EN, &mut en)?;
            en[0] &= !(1 << (id - 4));
            self.write_reg(REG_LDO_EN, &en)?;
        }
        Ok(())
    }

    /// Read register
    fn read_reg(&mut self, reg: u8, buf: &mut [u8]) -> Result<(), I2cError> {
        if let Some(i2c) = super::i2c::get_i2c(self.i2c_bus) {
            i2c.write_read(self.i2c_addr, reg, buf)
        } else {
            Err(I2cError::Timeout)
        }
    }

    /// Write register
    fn write_reg(&mut self, reg: u8, data: &[u8]) -> Result<(), I2cError> {
        let mut buf = [0u8; 9];
        buf[0] = reg;
        buf[1..1 + data.len()].copy_from_slice(data);
        
        if let Some(i2c) = super::i2c::get_i2c(self.i2c_bus) {
            i2c.write(self.i2c_addr, &buf[..1 + data.len()])
        } else {
            Err(I2cError::Timeout)
        }
    }
}

// Global PMIC instance
static mut PMIC: Option<PmicDriver> = None;

/// Initialize PMIC
pub fn init_pmic(i2c_bus: u8, i2c_addr: u8) {
    unsafe {
        let mut pmic = PmicDriver::new(i2c_bus, i2c_addr);
        if pmic.init().is_ok() {
            PMIC = Some(pmic);
        }
    }
}

/// Get PMIC driver
pub fn get_pmic() -> Option<&'static mut PmicDriver> {
    unsafe { PMIC.as_mut() }
}

/// Read battery level
pub fn battery_level() -> Option<u8> {
    get_pmic().and_then(|pmic| pmic.read_battery_level().ok())
}

/// Check if charging
pub fn is_charging() -> bool {
    get_pmic()
        .and_then(|pmic| pmic.is_charging().ok())
        .unwrap_or(false)
}
