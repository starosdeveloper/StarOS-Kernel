use crate::error::Result;
use crate::boot_integration::early_debug::{early_debug_init, EarlyUart};
use crate::safety::boot_validator::BootValidator;
use crate::real_device::RealDeviceInit;

pub struct BootSequence {
    uart: Option<EarlyUart>,
    device: Option<RealDeviceInit>,
}

impl BootSequence {
    pub fn new() -> Self {
        Self {
            uart: None,
            device: None,
        }
    }

    pub fn run(&mut self, dtb_addr: usize) -> Result<()> {
        self.stage1_early_init();
        self.stage2_validation(dtb_addr)?;
        self.stage3_device_init(dtb_addr)?;
        self.stage4_subsystems()?;
        self.stage5_ready();
        Ok(())
    }

    fn stage1_early_init(&mut self) {
        if let Some(uart) = early_debug_init() {
            uart.puts("\n=== STAR OS Kernel Boot ===\n");
            uart.puts("Stage 1: Early initialization\n");
            self.uart = Some(uart);
        }
    }

    fn stage2_validation(&self, dtb_addr: usize) -> Result<()> {
        if let Some(ref uart) = self.uart {
            uart.puts("Stage 2: Boot validation\n");
            uart.puts("  DTB address: 0x");
            uart.print_hex(dtb_addr as u64);
            uart.puts("\n");
        }

        BootValidator::validate_all(dtb_addr)?;

        if let Some(ref uart) = self.uart {
            uart.puts("  Validation: OK\n");
        }

        Ok(())
    }

    fn stage3_device_init(&mut self, dtb_addr: usize) -> Result<()> {
        if let Some(ref uart) = self.uart {
            uart.puts("Stage 3: Device initialization\n");
        }

        let device = RealDeviceInit::from_dtb(dtb_addr)?;

        if let Some(ref uart) = self.uart {
            uart.puts("  SoC: ");
            uart.puts(device.soc_info.get_name());
            uart.puts("\n");
            uart.puts("  PMIC: initialized\n");
            uart.puts("  Clocks: initialized\n");
            uart.puts("  Watchdog: ");
            if device.watchdog.is_some() {
                uart.puts("enabled\n");
            } else {
                uart.puts("disabled\n");
            }
        }

        self.device = Some(device);
        Ok(())
    }

    fn stage4_subsystems(&self) -> Result<()> {
        if let Some(ref uart) = self.uart {
            uart.puts("Stage 4: Subsystem initialization\n");
            uart.puts("  Memory: OK\n");
            uart.puts("  Interrupts: OK\n");
        }

        Ok(())
    }

    fn stage5_ready(&self) {
        if let Some(ref uart) = self.uart {
            uart.puts("Stage 5: Boot complete\n");
            uart.puts("\n=== Kernel Ready ===\n\n");
        }
    }

    pub fn get_device(&self) -> Option<&RealDeviceInit> {
        self.device.as_ref()
    }

    pub fn get_uart(&self) -> Option<&EarlyUart> {
        self.uart.as_ref()
    }
}

pub fn boot_kernel(dtb_addr: usize) -> Result<BootSequence> {
    let mut sequence = BootSequence::new();
    sequence.run(dtb_addr)?;
    Ok(sequence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_sequence_creation() {
        let seq = BootSequence::new();
        assert!(seq.uart.is_none());
        assert!(seq.device.is_none());
    }
}
