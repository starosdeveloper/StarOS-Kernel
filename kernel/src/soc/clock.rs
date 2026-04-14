use crate::error::{KernelError, Result};
use crate::prelude::*;
use crate::devicetree::parser::FdtParser;
use crate::devicetree::properties::{parse_reg, parse_u32};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]

#[cfg(feature = "std")]
use std::vec::Vec;

static CLOCK_INITIALIZED: AtomicBool = AtomicBool::new(false);
static GCC_BASE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClockId {
    UartClk,
    TimerClk,
    I2cClk,
    SpiClk,
    GpuClk,
    CpuClk,
    DisplayClk,
    UsbClk,
}

#[derive(Debug, Clone)]
pub struct ClockInfo {
    pub id: ClockId,
    pub offset: usize,
    pub parent_freq: u64,
    pub current_freq: u64,
    pub enabled: bool,
}

pub struct ClockController {
    base_addr: usize,
    clocks: Vec<ClockInfo>,
    source_freq: u64,
}

impl ClockController {
    pub fn from_device_tree(fdt: &FdtParser) -> Result<Self> {
        let mut gcc_addr = 0;
        let mut source_freq = 800_000_000;

        let _ = fdt.walk_nodes(|path, _offset, _depth| {
            if let Some(compatible) = fdt.read_property(path, "compatible") {
                let compat_str = core::str::from_utf8(compatible).unwrap_or("");
                
                if compat_str.contains("gcc") || 
                   compat_str.contains("clock-controller") ||
                   compat_str.contains("qcom,gcc") ||
                   compat_str.contains("mediatek,clk") {
                    
                    if let Some(reg) = fdt.read_property(path, "reg") {
                        if let Ok(regs) = parse_reg(reg, 2, 2) {
                            if let Some((addr, _size)) = regs.first() {
                                gcc_addr = *addr as usize;
                            }
                        }
                    }
                    
                    if let Some(freq_data) = fdt.read_property(path, "clock-frequency") {
                        if let Ok(freq) = parse_u32(freq_data) {
                            source_freq = freq as u64;
                        }
                    }
                }
                
                if compat_str.contains("fixed-clock") || compat_str.contains("xo") {
                    if let Some(freq_data) = fdt.read_property(path, "clock-frequency") {
                        if let Ok(freq) = parse_u32(freq_data) {
                            source_freq = freq as u64;
                        }
                    }
                }
            }
            Ok(true)
        });

        if gcc_addr == 0 {
            return Err(KernelError::NotFound);
        }

        let mut controller = Self {
            base_addr: gcc_addr,
            clocks: Vec::new(),
            source_freq,
        };

        controller.init_clocks()?;
        
        GCC_BASE.store(gcc_addr, Ordering::Release);
        CLOCK_INITIALIZED.store(true, Ordering::Release);
        
        Ok(controller)
    }

    fn init_clocks(&mut self) -> Result<()> {
        unsafe {
            core::ptr::write_volatile(self.base_addr as *mut u32, 0x1);
        }

        self.clocks.push(ClockInfo {
            id: ClockId::UartClk,
            offset: 0x1000,
            parent_freq: self.source_freq,
            current_freq: 0,
            enabled: false,
        });

        self.clocks.push(ClockInfo {
            id: ClockId::TimerClk,
            offset: 0x2000,
            parent_freq: self.source_freq,
            current_freq: 0,
            enabled: false,
        });

        self.clocks.push(ClockInfo {
            id: ClockId::I2cClk,
            offset: 0x3000,
            parent_freq: self.source_freq,
            current_freq: 0,
            enabled: false,
        });

        self.clocks.push(ClockInfo {
            id: ClockId::SpiClk,
            offset: 0x4000,
            parent_freq: self.source_freq,
            current_freq: 0,
            enabled: false,
        });

        self.clocks.push(ClockInfo {
            id: ClockId::GpuClk,
            offset: 0x5000,
            parent_freq: self.source_freq,
            current_freq: 0,
            enabled: false,
        });

        self.clocks.push(ClockInfo {
            id: ClockId::CpuClk,
            offset: 0x6000,
            parent_freq: self.source_freq,
            current_freq: 0,
            enabled: false,
        });

        self.clocks.push(ClockInfo {
            id: ClockId::DisplayClk,
            offset: 0x7000,
            parent_freq: self.source_freq,
            current_freq: 0,
            enabled: false,
        });

        self.clocks.push(ClockInfo {
            id: ClockId::UsbClk,
            offset: 0x8000,
            parent_freq: self.source_freq,
            current_freq: 0,
            enabled: false,
        });

        self.enable_clock(ClockId::UartClk)?;
        self.set_rate(ClockId::UartClk, 7372800)?;

        self.enable_clock(ClockId::TimerClk)?;
        self.set_rate(ClockId::TimerClk, 19200000)?;

        Ok(())
    }

    pub fn enable_clock(&mut self, id: ClockId) -> Result<()> {
        let clock = self.clocks.iter_mut()
            .find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        
        let offset = clock.offset;
        
        unsafe {
            let addr = (self.base_addr + offset) as *mut u32;
            let mut val = core::ptr::read_volatile(addr);
            val |= 0x1;
            core::ptr::write_volatile(addr, val);
        }

        for _ in 0..100 {
            unsafe {
                let addr = (self.base_addr + offset + 0x4) as *const u32;
                let status = core::ptr::read_volatile(addr);
                if status & 0x80000000 != 0 {
                    clock.enabled = true;
                    return Ok(());
                }
            }
            core::hint::spin_loop();
        }

        Err(KernelError::Timeout)
    }

    pub fn disable_clock(&mut self, id: ClockId) -> Result<()> {
        let clock = self.clocks.iter_mut()
            .find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        
        let offset = clock.offset;
        
        unsafe {
            let addr = (self.base_addr + offset) as *mut u32;
            let mut val = core::ptr::read_volatile(addr);
            val &= !0x1;
            core::ptr::write_volatile(addr, val);
        }

        clock.enabled = false;
        Ok(())
    }

    pub fn set_rate(&mut self, id: ClockId, rate_hz: u64) -> Result<()> {
        if !CLOCK_INITIALIZED.load(Ordering::Acquire) && 
           id != ClockId::UartClk && id != ClockId::TimerClk {
            return Err(KernelError::NotInitialized);
        }

        let divider = self.calculate_divider(rate_hz)?;
        
        let clock = self.clocks.iter_mut()
            .find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        
        let offset = clock.offset;

        unsafe {
            let addr = (self.base_addr + offset + 0x8) as *mut u32;
            core::ptr::write_volatile(addr, divider);
        }

        clock.current_freq = rate_hz;
        Ok(())
    }

    pub fn get_rate(&self, id: ClockId) -> Result<u64> {
        let clock = self.clocks.iter()
            .find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        
        if clock.current_freq > 0 {
            return Ok(clock.current_freq);
        }

        let offset = clock.offset;
        
        unsafe {
            let addr = (self.base_addr + offset + 0x8) as *const u32;
            let divider = core::ptr::read_volatile(addr);
            
            if divider == 0 {
                return Ok(0);
            }

            Ok(self.source_freq / divider as u64)
        }
    }

    pub fn is_enabled(&self, id: ClockId) -> bool {
        self.clocks.iter()
            .find(|c| c.id == id)
            .map(|c| c.enabled)
            .unwrap_or(false)
    }

    pub fn get_source_frequency(&self) -> u64 {
        self.source_freq
    }

    pub fn set_parent(&mut self, id: ClockId, parent_freq: u64) -> Result<()> {
        let clock = self.clocks.iter_mut()
            .find(|c| c.id == id)
            .ok_or(KernelError::NotFound)?;
        
        clock.parent_freq = parent_freq;
        Ok(())
    }

    fn calculate_divider(&self, rate_hz: u64) -> Result<u32> {
        if rate_hz == 0 || rate_hz > self.source_freq {
            return Err(KernelError::InvalidAddress);
        }

        let divider = self.source_freq / rate_hz;
        if divider > u32::MAX as u64 {
            return Err(KernelError::InvalidAddress);
        }

        Ok(divider as u32)
    }

    pub fn get_all_clocks(&self) -> &[ClockInfo] {
        &self.clocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_id_equality() {
        assert_eq!(ClockId::UartClk, ClockId::UartClk);
        assert_ne!(ClockId::UartClk, ClockId::TimerClk);
    }

    #[test]
    fn test_clock_info_creation() {
        let info = ClockInfo {
            id: ClockId::UartClk,
            offset: 0x1000,
            parent_freq: 800_000_000,
            current_freq: 7372800,
            enabled: true,
        };
        assert_eq!(info.id, ClockId::UartClk);
        assert_eq!(info.current_freq, 7372800);
        assert!(info.enabled);
    }

    #[test]
    fn test_calculate_divider() {
        let controller = ClockController {
            base_addr: 0,
            clocks: Vec::new(),
            source_freq: 800_000_000,
        };
        
        let div = controller.calculate_divider(19200000).unwrap();
        assert_eq!(div, 41);
        
        let div = controller.calculate_divider(7372800).unwrap();
        assert_eq!(div, 108);
    }

    #[test]
    fn test_calculate_divider_zero() {
        let controller = ClockController {
            base_addr: 0,
            clocks: Vec::new(),
            source_freq: 800_000_000,
        };
        assert!(controller.calculate_divider(0).is_err());
    }

    #[test]
    fn test_calculate_divider_too_high() {
        let controller = ClockController {
            base_addr: 0,
            clocks: Vec::new(),
            source_freq: 800_000_000,
        };
        assert!(controller.calculate_divider(1_000_000_000).is_err());
    }

    #[test]
    fn test_get_source_frequency() {
        let controller = ClockController {
            base_addr: 0,
            clocks: Vec::new(),
            source_freq: 800_000_000,
        };
        assert_eq!(controller.get_source_frequency(), 800_000_000);
    }
}
