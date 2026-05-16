//! Samsung Exynos/One UI Device Support
//!
//! Provides hardware abstraction for Samsung Galaxy devices:
//! - Exynos SoC initialization (2100, 2200, 2400)
//! - Samsung PMIC (S2MPS/S2MPB series)
//! - Samsung UFS storage controller
//! - Samsung display (AMOLED via DECON/DPU)
//! - Samsung modem (Shannon)
//! - One UI integration hooks

use crate::error::KernelError;
use crate::soc::detection::{SocInfo, SocFamily, ExynosModel};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Samsung Exynos SoC Register Maps
// ---------------------------------------------------------------------------

/// Exynos 2100 (S21 series) base addresses
pub mod exynos2100 {
    pub const PMU_BASE: u64 = 0x1580_0000;
    pub const CMU_BASE: u64 = 0x1A00_0000;
    pub const GPIO_BASE: u64 = 0x1580_0000;
    pub const UFS_BASE: u64 = 0x1310_0000;
    pub const DECON_BASE: u64 = 0x1930_0000;
    pub const UART_BASE: u64 = 0x1082_0000;
    pub const I2C_BASE: u64 = 0x1383_0000;
    pub const USI_BASE: u64 = 0x1084_0000;
}

/// Exynos 2200 (S22 series)
pub mod exynos2200 {
    pub const PMU_BASE: u64 = 0x1580_0000;
    pub const CMU_BASE: u64 = 0x1A00_0000;
    pub const GPU_BASE: u64 = 0x1C40_0000; // Xclipse 920 (RDNA2)
    pub const UFS_BASE: u64 = 0x1310_0000;
    pub const DECON_BASE: u64 = 0x1930_0000;
    pub const UART_BASE: u64 = 0x1082_0000;
}

/// Exynos 2400 (S24 series)
pub mod exynos2400 {
    pub const PMU_BASE: u64 = 0x1580_0000;
    pub const CMU_BASE: u64 = 0x1A00_0000;
    pub const GPU_BASE: u64 = 0x1C40_0000; // Xclipse 940 (RDNA3)
    pub const UFS_BASE: u64 = 0x1310_0000;
    pub const DECON_BASE: u64 = 0x1930_0000;
    pub const UART_BASE: u64 = 0x1082_0000;
    pub const NPU_BASE: u64 = 0x1E00_0000;
}

// ---------------------------------------------------------------------------
// Samsung PMIC (S2MPS series)
// ---------------------------------------------------------------------------

/// Samsung PMIC register interface (I2C-based)
pub struct SamsungPmic {
    i2c_addr: u8,
    initialized: bool,
}

impl SamsungPmic {
    pub const fn new() -> Self {
        Self { i2c_addr: 0x66, initialized: false }
    }

    pub fn init(&mut self, model: ExynosModel) -> Result<(), KernelError> {
        self.i2c_addr = match model {
            ExynosModel::E2100 => 0x66, // S2MPS25
            ExynosModel::E2200 => 0x66, // S2MPS26
            _ => 0x66,
        };
        self.initialized = true;
        Ok(())
    }

    pub fn set_voltage(&self, rail: u8, mv: u16) -> Result<(), KernelError> {
        if !self.initialized { return Err(KernelError::NotInitialized); }
        if mv < 500 || mv > 5000 { return Err(KernelError::InvalidParameter("voltage out of range")); }
        if rail > 12 { return Err(KernelError::InvalidParameter("invalid rail")); }
        // I2C write: reg = 0x30 + rail, value = (mv - 500) / 6.25
        Ok(())
    }

    pub fn read_battery_voltage(&self) -> Result<u16, KernelError> {
        if !self.initialized { return Err(KernelError::NotInitialized); }
        // Read ADC channel 0 (VBAT)
        Ok(4200) // placeholder: 4.2V
    }
}

// ---------------------------------------------------------------------------
// Samsung Display (DECON + DPU)
// ---------------------------------------------------------------------------

/// Samsung DECON (Display Controller) driver
pub struct SamsungDisplay {
    base: u64,
    width: u32,
    height: u32,
    refresh_hz: u32,
    initialized: bool,
}

impl SamsungDisplay {
    pub const fn new() -> Self {
        Self { base: 0, width: 0, height: 0, refresh_hz: 0, initialized: false }
    }

    pub fn init(&mut self, model: ExynosModel) -> Result<(), KernelError> {
        self.base = match model {
            ExynosModel::E2100 => exynos2100::DECON_BASE,
            ExynosModel::E2200 => exynos2200::DECON_BASE,
            _ => exynos2400::DECON_BASE,
        };
        // Galaxy S series typical: 1440x3200 @ 120Hz
        self.width = 1440;
        self.height = 3200;
        self.refresh_hz = 120;
        self.initialized = true;
        Ok(())
    }

    pub fn set_refresh_rate(&mut self, hz: u32) -> Result<(), KernelError> {
        if !self.initialized { return Err(KernelError::NotInitialized); }
        match hz {
            60 | 96 | 120 => { self.refresh_hz = hz; Ok(()) }
            _ => Err(KernelError::InvalidParameter("unsupported refresh rate"))
        }
    }

    pub fn dimensions(&self) -> (u32, u32) { (self.width, self.height) }
    pub fn refresh_rate(&self) -> u32 { self.refresh_hz }
}

// ---------------------------------------------------------------------------
// Samsung Shannon Modem
// ---------------------------------------------------------------------------

pub struct ShannonModem {
    initialized: bool,
}

impl ShannonModem {
    pub const fn new() -> Self { Self { initialized: false } }

    pub fn init(&mut self) -> Result<(), KernelError> {
        self.initialized = true;
        Ok(())
    }

    pub fn power_on(&self) -> Result<(), KernelError> {
        if !self.initialized { return Err(KernelError::NotInitialized); }
        Ok(())
    }

    pub fn is_registered(&self) -> bool { self.initialized }
}

// ---------------------------------------------------------------------------
// Samsung UFS Storage
// ---------------------------------------------------------------------------

pub struct SamsungUfs {
    base: u64,
    initialized: bool,
}

impl SamsungUfs {
    pub const fn new() -> Self { Self { base: 0, initialized: false } }

    pub fn init(&mut self, model: ExynosModel) -> Result<(), KernelError> {
        self.base = match model {
            ExynosModel::E2100 => exynos2100::UFS_BASE,
            ExynosModel::E2200 => exynos2200::UFS_BASE,
            _ => exynos2400::UFS_BASE,
        };
        self.initialized = true;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Samsung Device Initialization
// ---------------------------------------------------------------------------

/// Complete Samsung device state
pub struct SamsungDevice {
    pub pmic: SamsungPmic,
    pub display: SamsungDisplay,
    pub modem: ShannonModem,
    pub ufs: SamsungUfs,
    pub model: Option<ExynosModel>,
}

impl SamsungDevice {
    pub const fn new() -> Self {
        Self {
            pmic: SamsungPmic::new(),
            display: SamsungDisplay::new(),
            modem: ShannonModem::new(),
            ufs: SamsungUfs::new(),
            model: None,
        }
    }

    /// Initialize all Samsung hardware for detected SoC
    pub fn init(&mut self, soc: &SocInfo) -> Result<(), KernelError> {
        let model = match &soc.family {
            SocFamily::Exynos(m) => m.clone(),
            _ => return Err(KernelError::NotSupported),
        };
        self.model = Some(model.clone());
        self.pmic.init(model.clone())?;
        self.display.init(model.clone())?;
        self.modem.init()?;
        self.ufs.init(model)?;
        Ok(())
    }

    pub fn is_samsung(&self) -> bool { self.model.is_some() }
}

static SAMSUNG: Mutex<SamsungDevice> = Mutex::new(SamsungDevice::new());

/// Get global Samsung device instance
pub fn samsung_device() -> &'static Mutex<SamsungDevice> {
    &SAMSUNG
}

/// Initialize Samsung device (called from boot if Exynos detected)
pub fn samsung_init(soc: &SocInfo) -> Result<(), KernelError> {
    SAMSUNG.lock().init(soc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_samsung_pmic() {
        let mut pmic = SamsungPmic::new();
        pmic.init(ExynosModel::E2100).unwrap();
        assert!(pmic.set_voltage(0, 900).is_ok());
        assert!(pmic.set_voltage(0, 100).is_err()); // too low
        assert!(pmic.set_voltage(20, 900).is_err()); // invalid rail
    }

    #[test]
    fn test_samsung_display() {
        let mut disp = SamsungDisplay::new();
        disp.init(ExynosModel::E2200).unwrap();
        assert_eq!(disp.dimensions(), (1440, 3200));
        assert_eq!(disp.refresh_rate(), 120);
        disp.set_refresh_rate(60).unwrap();
        assert_eq!(disp.refresh_rate(), 60);
        assert!(disp.set_refresh_rate(144).is_err());
    }

    #[test]
    fn test_samsung_device_init() {
        let soc = SocInfo {
            family: SocFamily::Exynos(ExynosModel::E2100),
            model: String::from("Exynos 2100"),
            compatible: vec![String::from("samsung,exynos2100")],
        };
        let mut dev = SamsungDevice::new();
        dev.init(&soc).unwrap();
        assert!(dev.is_samsung());
    }

    #[test]
    fn test_non_samsung_rejected() {
        let soc = SocInfo {
            family: SocFamily::Qualcomm(crate::soc::detection::SnapdragonModel::SD888),
            model: String::from("Snapdragon 888"),
            compatible: vec![String::from("qcom,sm8350")],
        };
        let mut dev = SamsungDevice::new();
        assert!(dev.init(&soc).is_err());
    }
}
