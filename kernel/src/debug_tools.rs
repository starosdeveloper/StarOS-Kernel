use crate::boot_integration::early_debug::EarlyUart;
use crate::safety::memory_map::MemoryMap;
use crate::real_device::RealDeviceInit;

pub struct DebugLogger {
    uart: Option<EarlyUart>,
}

impl DebugLogger {
    pub fn new(uart: Option<EarlyUart>) -> Self {
        Self { uart }
    }

    pub fn log_boot_stage(&self, stage: u32, name: &str) {
        if let Some(ref uart) = self.uart {
            uart.puts("\n[Stage ");
            uart.print_dec(stage);
            uart.puts("] ");
            uart.puts(name);
            uart.puts("\n");
        }
    }

    pub fn log_device_info(&self, device: &RealDeviceInit) {
        if let Some(ref uart) = self.uart {
            uart.puts("\n=== Device Information ===\n");
            
            uart.puts("SoC: ");
            uart.puts(device.soc_info.get_name());
            uart.puts("\n");
            
            uart.puts("Compatible: ");
            for compat in device.soc_info.get_compatible_strings() {
                uart.puts(compat);
                uart.puts(" ");
            }
            uart.puts("\n");
            
            uart.puts("Watchdog: ");
            if device.watchdog.is_some() {
                uart.puts("enabled\n");
            } else {
                uart.puts("disabled\n");
            }
        }
    }

    pub fn log_memory_map(&self, memory_map: &MemoryMap) {
        if let Some(ref uart) = self.uart {
            uart.puts("\n=== Memory Map ===\n");
            
            for region in memory_map.get_safe_regions() {
                uart.puts("Safe:   0x");
                uart.print_hex(region.start.0);
                uart.puts(" - 0x");
                uart.print_hex(region.start.0 + region.size as u64);
                uart.puts(" (");
                uart.print_dec((region.size / 1024 / 1024) as u32);
                uart.puts(" MB) ");
                uart.puts(region.name);
                uart.puts("\n");
            }
        }
    }

    pub fn dump_registers(&self) {
        if let Some(ref uart) = self.uart {
            uart.puts("\n=== CPU Registers ===\n");
            
            unsafe {
                let mut el: u64;
                core::arch::asm!("mrs {}, CurrentEL", out(reg) el);
                uart.puts("CurrentEL: ");
                uart.print_dec(((el >> 2) & 0x3) as u32);
                uart.puts("\n");
                
                let mut sp: u64;
                core::arch::asm!("mov {}, sp", out(reg) sp);
                uart.puts("SP: 0x");
                uart.print_hex(sp);
                uart.puts("\n");
                
                let mut pc: u64;
                core::arch::asm!("adr {}, .", out(reg) pc);
                uart.puts("PC: 0x");
                uart.print_hex(pc);
                uart.puts("\n");
            }
        }
    }

    pub fn log_error(&self, error: &str) {
        if let Some(ref uart) = self.uart {
            uart.puts("\n❌ ERROR: ");
            uart.puts(error);
            uart.puts("\n");
        }
    }

    pub fn log_warning(&self, warning: &str) {
        if let Some(ref uart) = self.uart {
            uart.puts("\n⚠️  WARNING: ");
            uart.puts(warning);
            uart.puts("\n");
        }
    }

    pub fn log_success(&self, message: &str) {
        if let Some(ref uart) = self.uart {
            uart.puts("\n✅ SUCCESS: ");
            uart.puts(message);
            uart.puts("\n");
        }
    }

    pub fn log_hex(&self, label: &str, value: u64) {
        if let Some(ref uart) = self.uart {
            uart.puts(label);
            uart.puts(": 0x");
            uart.print_hex(value);
            uart.puts("\n");
        }
    }

    pub fn log_dec(&self, label: &str, value: u32) {
        if let Some(ref uart) = self.uart {
            uart.puts(label);
            uart.puts(": ");
            uart.print_dec(value);
            uart.puts("\n");
        }
    }
}

pub fn create_debug_logger(uart: Option<EarlyUart>) -> DebugLogger {
    DebugLogger::new(uart)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_logger_creation() {
        let logger = DebugLogger::new(None);
        logger.log_boot_stage(1, "Test");
    }
}
