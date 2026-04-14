const RESET_BASE: u64 = 0x00180000;

#[derive(Debug, Clone, Copy)]
pub enum ResetId {
    Uart = 0,
    I2c = 1,
    Spi = 2,
    Usb = 3,
    Gpu = 4,
}

pub struct ResetController {
    base_addr: u64,
}

impl ResetController {
    pub fn new() -> Self {
        Self {
            base_addr: RESET_BASE,
        }
    }

    pub fn assert_reset(&self, id: ResetId) {
        let offset = self.get_reset_offset(id);
        
        unsafe {
            let addr = (self.base_addr + offset) as *mut u32;
            let mut val = core::ptr::read_volatile(addr);
            val |= 0x1; // Assert reset
            core::ptr::write_volatile(addr, val);
        }
    }

    pub fn deassert_reset(&self, id: ResetId) {
        let offset = self.get_reset_offset(id);
        
        unsafe {
            let addr = (self.base_addr + offset) as *mut u32;
            let mut val = core::ptr::read_volatile(addr);
            val &= !0x1; // Deassert reset
            core::ptr::write_volatile(addr, val);
        }

        // Wait for reset to complete
        for _ in 0..100 {
            core::hint::spin_loop();
        }
    }

    pub fn reset_device(&self, id: ResetId) {
        self.assert_reset(id);
        
        // Hold reset for a bit
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        
        self.deassert_reset(id);
    }

    fn get_reset_offset(&self, id: ResetId) -> u64 {
        match id {
            ResetId::Uart => 0x100,
            ResetId::I2c => 0x200,
            ResetId::Spi => 0x300,
            ResetId::Usb => 0x400,
            ResetId::Gpu => 0x500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reset_controller_new() {
        let ctrl = ResetController::new();
        assert_eq!(ctrl.base_addr, RESET_BASE);
    }

    #[test]
    fn test_get_reset_offset() {
        let ctrl = ResetController::new();
        assert_eq!(ctrl.get_reset_offset(ResetId::Uart), 0x100);
        assert_eq!(ctrl.get_reset_offset(ResetId::I2c), 0x200);
    }

    #[test]
    fn test_assert_reset() {
        let ctrl = ResetController::new();
        ctrl.assert_reset(ResetId::Uart);
    }

    #[test]
    fn test_deassert_reset() {
        let ctrl = ResetController::new();
        ctrl.deassert_reset(ResetId::Uart);
    }

    #[test]
    fn test_reset_device() {
        let ctrl = ResetController::new();
        ctrl.reset_device(ResetId::Uart);
    }
}
