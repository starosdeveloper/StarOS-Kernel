use crate::error::{KernelError, Result};
use crate::prelude::*;
use super::parser::FdtParser;
use super::properties::{parse_reg, parse_u32, parse_string, parse_stringlist};

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]

#[cfg(feature = "std")]
use std::vec::Vec;

#[derive(Debug, Clone)]
pub struct UartInfo {
    pub base_addr: u64,
    pub size: u64,
    pub clock_frequency: u32,
    pub compatible: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TimerInfo {
    pub base_addr: u64,
    pub size: u64,
    pub interrupts: Vec<u32>,
    pub clock_frequency: u64,
}

#[derive(Debug, Clone)]
pub struct GicInfo {
    pub dist_base: u64,
    pub dist_size: u64,
    pub cpu_base: u64,
    pub cpu_size: u64,
}

#[derive(Debug, Clone)]
pub struct PmicInfo {
    pub base_addr: u64,
    pub size: u64,
    pub compatible: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ClockInfo {
    pub name: Vec<u8>,
    pub frequency: u64,
    pub phandle: u32,
}

pub struct DeviceDiscovery<'a> {
    parser: &'a FdtParser,
}

impl<'a> DeviceDiscovery<'a> {
    pub fn new(parser: &'a FdtParser) -> Self {
        Self { parser }
    }

    pub fn find_uart(&self) -> Option<UartInfo> {
        let mut result = None;
        
        let _ = self.parser.walk_nodes(|path, _offset, _depth| {
            if let Some(compatible) = self.parser.read_property(path, "compatible") {
                let compat_str = core::str::from_utf8(compatible).unwrap_or("");
                
                if compat_str.contains("uart") || 
                   compat_str.contains("serial") ||
                   compat_str.contains("pl011") ||
                   compat_str.contains("8250") {
                    
                    if let Some(reg) = self.parser.read_property(path, "reg") {
                        if let Ok(regs) = parse_reg(reg, 2, 2) {
                            if let Some((addr, size)) = regs.first() {
                                let clock_freq = self.parser.read_property(path, "clock-frequency")
                                    .and_then(|data| parse_u32(data).ok())
                                    .unwrap_or(24000000);
                                
                                result = Some(UartInfo {
                                    base_addr: *addr,
                                    size: *size,
                                    clock_frequency: clock_freq,
                                    compatible: compatible.to_vec(),
                                });
                                
                                return Ok(false);
                            }
                        }
                    }
                }
            }
            Ok(true)
        });
        
        result
    }

    pub fn find_timer(&self) -> Option<TimerInfo> {
        let mut result = None;
        
        let _ = self.parser.walk_nodes(|path, _offset, _depth| {
            if let Some(compatible) = self.parser.read_property(path, "compatible") {
                let compat_str = core::str::from_utf8(compatible).unwrap_or("");
                
                if compat_str.contains("timer") ||
                   compat_str.contains("arm,armv8-timer") ||
                   compat_str.contains("arm,armv7-timer") {
                    
                    if let Some(reg) = self.parser.read_property(path, "reg") {
                        if let Ok(regs) = parse_reg(reg, 2, 2) {
                            if let Some((addr, size)) = regs.first() {
                                let interrupts = self.parser.read_property(path, "interrupts")
                                    .and_then(|data| super::properties::parse_interrupts(data).ok())
                                    .unwrap_or_default();
                                
                                let clock_freq = self.parser.read_property(path, "clock-frequency")
                                    .and_then(|data| parse_u32(data).ok())
                                    .unwrap_or(19200000) as u64;
                                
                                result = Some(TimerInfo {
                                    base_addr: *addr,
                                    size: *size,
                                    interrupts,
                                    clock_frequency: clock_freq,
                                });
                                
                                return Ok(false);
                            }
                        }
                    }
                }
            }
            Ok(true)
        });
        
        result
    }

    pub fn find_gic(&self) -> Option<GicInfo> {
        let mut result = None;
        
        let _ = self.parser.walk_nodes(|path, _offset, _depth| {
            if let Some(compatible) = self.parser.read_property(path, "compatible") {
                let compat_str = core::str::from_utf8(compatible).unwrap_or("");
                
                if compat_str.contains("gic") ||
                   compat_str.contains("arm,cortex-a") {
                    
                    if let Some(reg) = self.parser.read_property(path, "reg") {
                        if let Ok(regs) = parse_reg(reg, 2, 2) {
                            if regs.len() >= 2 {
                                result = Some(GicInfo {
                                    dist_base: regs[0].0,
                                    dist_size: regs[0].1,
                                    cpu_base: regs[1].0,
                                    cpu_size: regs[1].1,
                                });
                                
                                return Ok(false);
                            }
                        }
                    }
                }
            }
            Ok(true)
        });
        
        result
    }

    pub fn find_pmic(&self) -> Option<PmicInfo> {
        let mut result = None;
        
        let _ = self.parser.walk_nodes(|path, _offset, _depth| {
            if let Some(compatible) = self.parser.read_property(path, "compatible") {
                let compat_str = core::str::from_utf8(compatible).unwrap_or("");
                
                if compat_str.contains("pmic") ||
                   compat_str.contains("pm8") ||
                   compat_str.contains("pm6") ||
                   compat_str.contains("spmi") {
                    
                    if let Some(reg) = self.parser.read_property(path, "reg") {
                        if let Ok(regs) = parse_reg(reg, 2, 2) {
                            if let Some((addr, size)) = regs.first() {
                                result = Some(PmicInfo {
                                    base_addr: *addr,
                                    size: *size,
                                    compatible: compatible.to_vec(),
                                });
                                
                                return Ok(false);
                            }
                        }
                    }
                }
            }
            Ok(true)
        });
        
        result
    }

    pub fn find_clocks(&self) -> Vec<ClockInfo> {
        let mut clocks = Vec::new();
        
        let _ = self.parser.walk_nodes(|path, _offset, _depth| {
            if let Some(compatible) = self.parser.read_property(path, "compatible") {
                let compat_str = core::str::from_utf8(compatible).unwrap_or("");
                
                if compat_str.contains("clock") ||
                   compat_str.contains("fixed-clock") ||
                   compat_str.contains("gcc") {
                    
                    let frequency = self.parser.read_property(path, "clock-frequency")
                        .and_then(|data| parse_u32(data).ok())
                        .unwrap_or(0) as u64;
                    
                    let phandle = self.parser.read_property(path, "phandle")
                        .and_then(|data| parse_u32(data).ok())
                        .unwrap_or(0);
                    
                    let name = self.parser.read_property(path, "clock-output-names")
                        .or_else(|| self.parser.read_property(path, "name"))
                        .map(|data| data.to_vec())
                        .unwrap_or_else(|| path.as_bytes().to_vec());
                    
                    clocks.push(ClockInfo {
                        name,
                        frequency,
                        phandle,
                    });
                }
            }
            Ok(true)
        });
        
        clocks
    }

    pub fn find_memory_regions(&self) -> Vec<(u64, u64)> {
        let mut regions = Vec::new();
        
        let _ = self.parser.walk_nodes(|path, _offset, _depth| {
            if path.starts_with("memory") || path.contains("/memory@") {
                if let Some(reg) = self.parser.read_property(path, "reg") {
                    if let Ok(regs) = parse_reg(reg, 2, 2) {
                        regions.extend(regs);
                    }
                }
            }
            Ok(true)
        });
        
        regions
    }

    pub fn find_reserved_memory(&self) -> Vec<(u64, u64)> {
        let mut regions = Vec::new();
        
        let _ = self.parser.walk_nodes(|path, _offset, _depth| {
            if path.contains("reserved-memory") {
                if let Some(reg) = self.parser.read_property(path, "reg") {
                    if let Ok(regs) = parse_reg(reg, 2, 2) {
                        regions.extend(regs);
                    }
                }
            }
            Ok(true)
        });
        
        regions
    }

    pub fn get_model(&self) -> Option<&str> {
        self.parser.read_property("/", "model")
            .and_then(|data| parse_string(data).ok())
    }

    pub fn get_compatible(&self) -> Option<Vec<&str>> {
        self.parser.read_property("/", "compatible")
            .and_then(|data| parse_stringlist(data).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_discovery_creation() {
        // Test requires valid FDT
    }

    #[test]
    fn test_uart_info_creation() {
        let info = UartInfo {
            base_addr: 0x0A84000,
            size: 0x1000,
            clock_frequency: 7372800,
            compatible: b"arm,pl011".to_vec(),
        };
        assert_eq!(info.base_addr, 0x0A84000);
        assert_eq!(info.clock_frequency, 7372800);
    }

    #[test]
    fn test_timer_info_creation() {
        let info = TimerInfo {
            base_addr: 0x09020000,
            size: 0x1000,
            interrupts: vec![1, 2, 3],
            clock_frequency: 19200000,
        };
        assert_eq!(info.base_addr, 0x09020000);
        assert_eq!(info.interrupts.len(), 3);
    }

    #[test]
    fn test_gic_info_creation() {
        let info = GicInfo {
            dist_base: 0x08000000,
            dist_size: 0x10000,
            cpu_base: 0x08010000,
            cpu_size: 0x10000,
        };
        assert_eq!(info.dist_base, 0x08000000);
        assert_eq!(info.cpu_base, 0x08010000);
    }

    #[test]
    fn test_pmic_info_creation() {
        let info = PmicInfo {
            base_addr: 0x0C000000,
            size: 0x1000000,
            compatible: b"qcom,pm8998".to_vec(),
        };
        assert_eq!(info.base_addr, 0x0C000000);
    }

    #[test]
    fn test_clock_info_creation() {
        let info = ClockInfo {
            name: b"xo_board".to_vec(),
            frequency: 19200000,
            phandle: 1,
        };
        assert_eq!(info.frequency, 19200000);
        assert_eq!(info.phandle, 1);
    }
}
