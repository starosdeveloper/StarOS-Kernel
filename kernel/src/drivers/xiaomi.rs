//! Xiaomi Device Support — Full Lineup
//!
//! Native support for Xiaomi/Redmi/POCO devices:
//! - **Xiaomi** (flagship): Xiaomi 13, 14, 14 Ultra
//! - **Redmi Note** (mid-range): Note 12/13 Pro, Note 13 Pro+
//! - **Redmi** (budget): Redmi 12, 13, A3
//! - **POCO** (performance): POCO F5, F6, X5 Pro, X6 Pro
//!
//! SoC coverage:
//! - Snapdragon: 4 Gen 2, 685, 7s Gen 2, 7+ Gen 2, 8 Gen 2/3
//! - MediaTek: Helio G88/G99, Dimensity 1080, 7200, 8100, 9200+

use crate::error::KernelError;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Xiaomi Device Categories
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XiaomiTier {
    /// Redmi A / Redmi number series
    Budget,
    /// Redmi Note / POCO X series
    MidRange,
    /// Xiaomi number / POCO F series
    Flagship,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XiaomiModel {
    // Budget - Redmi
    RedmiA3,        // MediaTek Helio G36
    Redmi12,        // MediaTek Helio G88
    Redmi13,        // Snapdragon 685
    RedmiNote12,    // Snapdragon 685
    RedmiNote13,    // Snapdragon 685

    // Mid-range - Redmi Note Pro / POCO X
    RedmiNote12Pro,  // MediaTek Dimensity 1080
    RedmiNote13Pro,  // Snapdragon 7s Gen 2
    RedmiNote13ProPlus, // MediaTek Dimensity 7200
    PocoX5Pro,       // Snapdragon 778G
    PocoX6Pro,       // MediaTek Dimensity 8300

    // Flagship - Xiaomi / POCO F
    Xiaomi13,        // Snapdragon 8 Gen 2
    Xiaomi14,        // Snapdragon 8 Gen 3
    Xiaomi14Ultra,   // Snapdragon 8 Gen 3
    PocoF5,          // Snapdragon 7+ Gen 2
    PocoF6,          // Snapdragon 8s Gen 3
    PocoF6Pro,       // Snapdragon 8 Gen 2
}

impl XiaomiModel {
    pub fn tier(&self) -> XiaomiTier {
        match self {
            Self::RedmiA3 | Self::Redmi12 | Self::Redmi13 |
            Self::RedmiNote12 | Self::RedmiNote13 => XiaomiTier::Budget,

            Self::RedmiNote12Pro | Self::RedmiNote13Pro |
            Self::RedmiNote13ProPlus | Self::PocoX5Pro |
            Self::PocoX6Pro => XiaomiTier::MidRange,

            _ => XiaomiTier::Flagship,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::RedmiA3 => "Redmi A3",
            Self::Redmi12 => "Redmi 12",
            Self::Redmi13 => "Redmi 13",
            Self::RedmiNote12 => "Redmi Note 12",
            Self::RedmiNote13 => "Redmi Note 13",
            Self::RedmiNote12Pro => "Redmi Note 12 Pro",
            Self::RedmiNote13Pro => "Redmi Note 13 Pro",
            Self::RedmiNote13ProPlus => "Redmi Note 13 Pro+",
            Self::PocoX5Pro => "POCO X5 Pro",
            Self::PocoX6Pro => "POCO X6 Pro",
            Self::Xiaomi13 => "Xiaomi 13",
            Self::Xiaomi14 => "Xiaomi 14",
            Self::Xiaomi14Ultra => "Xiaomi 14 Ultra",
            Self::PocoF5 => "POCO F5",
            Self::PocoF6 => "POCO F6",
            Self::PocoF6Pro => "POCO F6 Pro",
        }
    }

    pub fn soc_name(&self) -> &'static str {
        match self {
            Self::RedmiA3 => "MediaTek Helio G36",
            Self::Redmi12 => "MediaTek Helio G88",
            Self::Redmi13 => "Snapdragon 685",
            Self::RedmiNote12 => "Snapdragon 685",
            Self::RedmiNote13 => "Snapdragon 685",
            Self::RedmiNote12Pro => "Dimensity 1080",
            Self::RedmiNote13Pro => "Snapdragon 7s Gen 2",
            Self::RedmiNote13ProPlus => "Dimensity 7200",
            Self::PocoX5Pro => "Snapdragon 778G",
            Self::PocoX6Pro => "Dimensity 8300",
            Self::Xiaomi13 => "Snapdragon 8 Gen 2",
            Self::Xiaomi14 => "Snapdragon 8 Gen 3",
            Self::Xiaomi14Ultra => "Snapdragon 8 Gen 3",
            Self::PocoF5 => "Snapdragon 7+ Gen 2",
            Self::PocoF6 => "Snapdragon 8s Gen 3",
            Self::PocoF6Pro => "Snapdragon 8 Gen 2",
        }
    }

    pub fn display_resolution(&self) -> (u32, u32) {
        match self.tier() {
            XiaomiTier::Budget => (720, 1600),     // HD+
            XiaomiTier::MidRange => (1080, 2400),  // FHD+ AMOLED
            XiaomiTier::Flagship => (1440, 3200),  // QHD+ LTPO
        }
    }

    pub fn max_refresh_hz(&self) -> u32 {
        match self {
            Self::RedmiA3 | Self::Redmi12 => 60,
            Self::Redmi13 | Self::RedmiNote12 | Self::RedmiNote13 => 90,
            _ => 120,
        }
    }

    pub fn has_nfc(&self) -> bool {
        matches!(self.tier(), XiaomiTier::MidRange | XiaomiTier::Flagship)
    }

    pub fn has_ir_blaster(&self) -> bool {
        // Most Xiaomi devices have IR blaster
        !matches!(self, Self::RedmiA3)
    }
}

// ---------------------------------------------------------------------------
// Xiaomi Hardware Abstraction
// ---------------------------------------------------------------------------

/// Xiaomi-specific hardware features
pub struct XiaomiDevice {
    pub model: Option<XiaomiModel>,
    pub initialized: bool,
}

impl XiaomiDevice {
    pub const fn new() -> Self {
        Self { model: None, initialized: false }
    }

    pub fn init(&mut self, model: XiaomiModel) -> Result<(), KernelError> {
        self.model = Some(model);
        self.initialized = true;
        Ok(())
    }

    pub fn is_xiaomi(&self) -> bool {
        self.model.is_some()
    }
}

static XIAOMI: Mutex<XiaomiDevice> = Mutex::new(XiaomiDevice::new());

pub fn xiaomi_device() -> &'static Mutex<XiaomiDevice> {
    &XIAOMI
}

pub fn xiaomi_init(model: XiaomiModel) -> Result<(), KernelError> {
    XIAOMI.lock().init(model)
}

/// Get list of all supported Xiaomi models
pub fn supported_models() -> &'static [XiaomiModel] {
    &[
        XiaomiModel::RedmiA3, XiaomiModel::Redmi12, XiaomiModel::Redmi13,
        XiaomiModel::RedmiNote12, XiaomiModel::RedmiNote13,
        XiaomiModel::RedmiNote12Pro, XiaomiModel::RedmiNote13Pro,
        XiaomiModel::RedmiNote13ProPlus, XiaomiModel::PocoX5Pro, XiaomiModel::PocoX6Pro,
        XiaomiModel::Xiaomi13, XiaomiModel::Xiaomi14, XiaomiModel::Xiaomi14Ultra,
        XiaomiModel::PocoF5, XiaomiModel::PocoF6, XiaomiModel::PocoF6Pro,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tiers() {
        assert_eq!(XiaomiModel::RedmiA3.tier(), XiaomiTier::Budget);
        assert_eq!(XiaomiModel::RedmiNote13Pro.tier(), XiaomiTier::MidRange);
        assert_eq!(XiaomiModel::Xiaomi14.tier(), XiaomiTier::Flagship);
    }

    #[test]
    fn test_display() {
        assert_eq!(XiaomiModel::Redmi12.display_resolution(), (720, 1600));
        assert_eq!(XiaomiModel::Xiaomi14.display_resolution(), (1440, 3200));
    }

    #[test]
    fn test_init() {
        let mut dev = XiaomiDevice::new();
        dev.init(XiaomiModel::PocoF6).unwrap();
        assert!(dev.is_xiaomi());
    }
}
