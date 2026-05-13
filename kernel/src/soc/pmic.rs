use crate::error::{KernelError, Result};
use crate::devicetree::parser::FdtParser;
use crate::devicetree::properties::parse_reg;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static PMIC_INITIALIZED: AtomicBool = AtomicBool::new(false);
static PMIC_BASE: AtomicUsize = AtomicUsize::new(0);
static SPMI_BASE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PmicType {
    Qualcomm(QualcommPmic),
    MediaTek(MediaTekPmic),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QualcommPmic {
    PM8998,
    PM8150,
    PM8250,
    PM6125,
    PM660,
    PM8916,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaTekPmic {
    MT6358,
    MT6359,
    MT6360,
}

pub struct PmicDriver {
    base_addr: usize,
    spmi_addr: usize,
    pmic_type: PmicType,
}

impl PmicDriver {
    pub fn from_device_tree(fdt: &FdtParser) -> Result<Self> {
        let mut pmic_addr = 0;
        let mut spmi_addr = 0;
        let mut pmic_type = PmicType::Unknown;

        let _ = fdt.walk_nodes(|path, _offset, _depth| {
            if let Some(compatible) = fdt.read_property(path, "compatible") {
                let compat_str = core::str::from_utf8(compatible).unwrap_or("");
                
                // Detect SPMI controller
                if compat_str.contains("spmi") || compat_str.contains("qcom,spmi-pmic-arb") {
                    if let Some(reg) = fdt.read_property(path, "reg") {
                        if let Ok(regs) = parse_reg(reg, 2, 2) {
                            if let Some((addr, _size)) = regs.first() {
                                spmi_addr = *addr as usize;
                            }
                        }
                    }
                }
                
                // Detect PMIC type
                if compat_str.contains("qcom,pm8998") {
                    pmic_type = PmicType::Qualcomm(QualcommPmic::PM8998);
                } else if compat_str.contains("qcom,pm8150") {
                    pmic_type = PmicType::Qualcomm(QualcommPmic::PM8150);
                } else if compat_str.contains("qcom,pm8250") {
                    pmic_type = PmicType::Qualcomm(QualcommPmic::PM8250);
                } else if compat_str.contains("qcom,pm6125") {
                    pmic_type = PmicType::Qualcomm(QualcommPmic::PM6125);
                } else if compat_str.contains("qcom,pm660") {
                    pmic_type = PmicType::Qualcomm(QualcommPmic::PM660);
                } else if compat_str.contains("qcom,pm8916") {
                    pmic_type = PmicType::Qualcomm(QualcommPmic::PM8916);
                } else if compat_str.contains("mediatek,mt6358") {
                    pmic_type = PmicType::MediaTek(MediaTekPmic::MT6358);
                } else if compat_str.contains("mediatek,mt6359") {
                    pmic_type = PmicType::MediaTek(MediaTekPmic::MT6359);
                } else if compat_str.contains("mediatek,mt6360") {
                    pmic_type = PmicType::MediaTek(MediaTekPmic::MT6360);
                }
                
                // Get PMIC base address
                if pmic_type != PmicType::Unknown && pmic_addr == 0 {
                    if let Some(reg) = fdt.read_property(path, "reg") {
                        if let Ok(regs) = parse_reg(reg, 2, 2) {
                            if let Some((addr, _size)) = regs.first() {
                                pmic_addr = *addr as usize;
                            }
                        }
                    }
                }
            }
            Ok(true)
        });

        if pmic_addr == 0 {
            return Err(KernelError::NotFound);
        }

        let driver = Self {
            base_addr: pmic_addr,
            spmi_addr,
            pmic_type,
        };

        driver.init_hardware()?;
        
        PMIC_BASE.store(pmic_addr, Ordering::Release);
        SPMI_BASE.store(spmi_addr, Ordering::Release);
        PMIC_INITIALIZED.store(true, Ordering::Release);
        
        Ok(driver)
    }

    fn init_hardware(&self) -> Result<()> {
        match self.pmic_type {
            PmicType::Qualcomm(_) => self.init_qualcomm(),
            PmicType::MediaTek(_) => self.init_mediatek(),
            PmicType::Unknown => Err(KernelError::NotSupported),
        }
    }

    fn init_qualcomm(&self) -> Result<()> {
        if self.spmi_addr != 0 {
            unsafe {
                core::ptr::write_volatile(self.spmi_addr as *mut u32, 0x1);
                
                for _ in 0..1000 {
                    let status = core::ptr::read_volatile((self.spmi_addr + 0x4) as *const u32);
                    if status & 0x1 != 0 {
                        break;
                    }
                    core::hint::spin_loop();
                }
            }
        }

        self.enable_rail("vdd_cx")?;
        self.enable_rail("vdd_mx")?;
        
        Ok(())
    }

    fn init_mediatek(&self) -> Result<()> {
        unsafe {
            core::ptr::write_volatile(self.base_addr as *mut u32, 0x1);
        }
        
        self.enable_rail("vcore")?;
        self.enable_rail("vproc")?;
        
        Ok(())
    }

    pub fn set_voltage(&self, rail: &str, voltage_mv: u32) -> Result<()> {
        if !PMIC_INITIALIZED.load(Ordering::Acquire) {
            return Err(KernelError::NotInitialized);
        }

        // Reject voltages outside safe hardware range to prevent damage
        if voltage_mv < 500 || voltage_mv > 5000 {
            return Err(KernelError::InvalidAddress);
        }

        let rail_offset = self.get_rail_offset(rail)?;
        
        match self.pmic_type {
            PmicType::Qualcomm(_) => {
                let voltage_reg = voltage_mv / 10;
                unsafe {
                    let addr = (self.base_addr + rail_offset + 0x40) as *mut u32;
                    core::ptr::write_volatile(addr, voltage_reg);
                }
            }
            PmicType::MediaTek(_) => {
                let voltage_reg = (voltage_mv - 500) / 6250;
                unsafe {
                    let addr = (self.base_addr + rail_offset + 0x10) as *mut u32;
                    core::ptr::write_volatile(addr, voltage_reg);
                }
            }
            PmicType::Unknown => return Err(KernelError::NotSupported),
        }

        Ok(())
    }

    pub fn enable_rail(&self, rail: &str) -> Result<()> {
        let rail_offset = self.get_rail_offset(rail)?;

        match self.pmic_type {
            PmicType::Qualcomm(_) => {
                unsafe {
                    let addr = (self.base_addr + rail_offset + 0x46) as *mut u32;
                    core::ptr::write_volatile(addr, 0x80);
                }
            }
            PmicType::MediaTek(_) => {
                unsafe {
                    let addr = (self.base_addr + rail_offset) as *mut u32;
                    let mut val = core::ptr::read_volatile(addr);
                    val |= 0x1;
                    core::ptr::write_volatile(addr, val);
                }
            }
            PmicType::Unknown => return Err(KernelError::NotSupported),
        }

        for _ in 0..1000 {
            core::hint::spin_loop();
        }

        Ok(())
    }

    pub fn disable_rail(&self, rail: &str) -> Result<()> {
        let rail_offset = self.get_rail_offset(rail)?;

        match self.pmic_type {
            PmicType::Qualcomm(_) => {
                unsafe {
                    let addr = (self.base_addr + rail_offset + 0x46) as *mut u32;
                    core::ptr::write_volatile(addr, 0x00);
                }
            }
            PmicType::MediaTek(_) => {
                unsafe {
                    let addr = (self.base_addr + rail_offset) as *mut u32;
                    let mut val = core::ptr::read_volatile(addr);
                    val &= !0x1;
                    core::ptr::write_volatile(addr, val);
                }
            }
            PmicType::Unknown => return Err(KernelError::NotSupported),
        }

        Ok(())
    }

    pub fn read_battery(&self) -> u32 {
        match self.pmic_type {
            PmicType::Qualcomm(_) => unsafe {
                let addr = (self.base_addr + 0x3100) as *const u32;
                core::ptr::read_volatile(addr)
            },
            PmicType::MediaTek(_) => unsafe {
                let addr = (self.base_addr + 0x1000) as *const u32;
                core::ptr::read_volatile(addr)
            },
            PmicType::Unknown => 0,
        }
    }

    pub fn read_voltage(&self, rail: &str) -> Result<u32> {
        let rail_offset = self.get_rail_offset(rail)?;

        match self.pmic_type {
            PmicType::Qualcomm(_) => unsafe {
                let addr = (self.base_addr + rail_offset + 0x40) as *const u32;
                let reg_val = core::ptr::read_volatile(addr);
                Ok(reg_val * 10)
            },
            PmicType::MediaTek(_) => unsafe {
                let addr = (self.base_addr + rail_offset + 0x10) as *const u32;
                let reg_val = core::ptr::read_volatile(addr);
                Ok(500 + reg_val * 6250 / 1000)
            },
            PmicType::Unknown => Err(KernelError::NotSupported),
        }
    }

    fn get_rail_offset(&self, rail: &str) -> Result<usize> {
        match self.pmic_type {
            PmicType::Qualcomm(_) => match rail {
                "vdd_cx" => Ok(0x1400),
                "vdd_mx" => Ok(0x1500),
                "vdd_lpi_cx" => Ok(0x1600),
                "vdd_lpi_mx" => Ok(0x1700),
                "vdd_gfx" => Ok(0x1800),
                "vdd_mss" => Ok(0x1900),
                _ => Err(KernelError::NotFound),
            },
            PmicType::MediaTek(_) => match rail {
                "vcore" => Ok(0x200),
                "vproc" => Ok(0x300),
                "vgpu" => Ok(0x400),
                "vmodem" => Ok(0x500),
                _ => Err(KernelError::NotFound),
            },
            PmicType::Unknown => Err(KernelError::NotSupported),
        }
    }

    pub fn get_type(&self) -> PmicType {
        self.pmic_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pmic_type_detection() {
        assert_eq!(
            PmicType::Qualcomm(QualcommPmic::PM8998),
            PmicType::Qualcomm(QualcommPmic::PM8998)
        );
    }

    #[test]
    fn test_rail_offset_qualcomm() {
        let pmic = PmicDriver {
            base_addr: 0,
            spmi_addr: 0,
            pmic_type: PmicType::Qualcomm(QualcommPmic::PM8998),
        };
        assert_eq!(pmic.get_rail_offset("vdd_cx").unwrap(), 0x1400);
        assert_eq!(pmic.get_rail_offset("vdd_mx").unwrap(), 0x1500);
    }

    #[test]
    fn test_rail_offset_mediatek() {
        let pmic = PmicDriver {
            base_addr: 0,
            spmi_addr: 0,
            pmic_type: PmicType::MediaTek(MediaTekPmic::MT6358),
        };
        assert_eq!(pmic.get_rail_offset("vcore").unwrap(), 0x200);
        assert_eq!(pmic.get_rail_offset("vproc").unwrap(), 0x300);
    }

    #[test]
    fn test_invalid_rail() {
        let pmic = PmicDriver {
            base_addr: 0,
            spmi_addr: 0,
            pmic_type: PmicType::Qualcomm(QualcommPmic::PM8998),
        };
        assert!(pmic.get_rail_offset("invalid").is_err());
    }

    #[test]
    fn test_unknown_pmic() {
        let pmic = PmicDriver {
            base_addr: 0,
            spmi_addr: 0,
            pmic_type: PmicType::Unknown,
        };
        assert!(pmic.get_rail_offset("vdd_cx").is_err());
    }
}
