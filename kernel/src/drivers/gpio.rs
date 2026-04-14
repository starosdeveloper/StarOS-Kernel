//! GPIO driver
//!
//! Generic GPIO controller for buttons, LEDs, etc.

use crate::error::KernelError;

/// GPIO pin direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioDirection {
    Input,
    Output,
}

/// GPIO pin value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioValue {
    Low = 0,
    High = 1,
}

/// GPIO pin
pub struct GpioPin {
    base: usize,
    pin: u32,
    direction: GpioDirection,
}

impl GpioPin {
    pub const fn new(base: usize, pin: u32) -> Self {
        Self {
            base,
            pin,
            direction: GpioDirection::Input,
        }
    }

    /// Set pin direction
    pub fn set_direction(&mut self, direction: GpioDirection) -> Result<(), KernelError> {
        self.direction = direction;
        
        #[cfg(not(feature = "std"))]
        unsafe {
            // Write to GPIO direction register
            let dir_reg = (self.base + 0x400) as *mut u32;
            let mut val = core::ptr::read_volatile(dir_reg);
            
            match direction {
                GpioDirection::Output => val |= 1 << self.pin,
                GpioDirection::Input => val &= !(1 << self.pin),
            }
            
            core::ptr::write_volatile(dir_reg, val);
        }

        Ok(())
    }

    /// Set pin value (for output pins)
    pub fn set(&self, value: GpioValue) -> Result<(), KernelError> {
        if self.direction != GpioDirection::Output {
            return Err(KernelError::InvalidParameter("Pin not configured as output"));
        }

        #[cfg(not(feature = "std"))]
        unsafe {
            let data_reg = (self.base + (4 << self.pin)) as *mut u32;
            core::ptr::write_volatile(data_reg, value as u32);
        }

        Ok(())
    }

    /// Get pin value
    pub fn get(&self) -> GpioValue {
        #[cfg(not(feature = "std"))]
        unsafe {
            let data_reg = (self.base + (4 << self.pin)) as *const u32;
            let val = core::ptr::read_volatile(data_reg);
            if val != 0 {
                GpioValue::High
            } else {
                GpioValue::Low
            }
        }

        #[cfg(feature = "std")]
        GpioValue::Low
    }

    /// Toggle pin (for output pins)
    pub fn toggle(&self) -> Result<(), KernelError> {
        let current = self.get();
        let new_value = match current {
            GpioValue::Low => GpioValue::High,
            GpioValue::High => GpioValue::Low,
        };
        self.set(new_value)
    }

    pub fn pin_number(&self) -> u32 {
        self.pin
    }

    pub fn direction(&self) -> GpioDirection {
        self.direction
    }
}

unsafe impl Send for GpioPin {}
unsafe impl Sync for GpioPin {}

/// GPIO controller
pub struct GpioController {
    base: usize,
    num_pins: u32,
}

impl GpioController {
    pub const fn new(base: usize, num_pins: u32) -> Self {
        Self { base, num_pins }
    }

    /// Get GPIO pin
    pub fn pin(&self, pin: u32) -> Result<GpioPin, KernelError> {
        if pin >= self.num_pins {
            return Err(KernelError::InvalidParameter("Invalid pin number"));
        }

        Ok(GpioPin::new(self.base, pin))
    }

    pub fn num_pins(&self) -> u32 {
        self.num_pins
    }
}

unsafe impl Send for GpioController {}
unsafe impl Sync for GpioController {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpio_pin() {
        let mut pin = GpioPin::new(0x1000, 5);
        assert_eq!(pin.pin_number(), 5);
        assert_eq!(pin.direction(), GpioDirection::Input);

        pin.set_direction(GpioDirection::Output).unwrap();
        assert_eq!(pin.direction(), GpioDirection::Output);
    }

    #[test]
    fn test_gpio_value() {
        assert_eq!(GpioValue::Low as u32, 0);
        assert_eq!(GpioValue::High as u32, 1);
    }

    #[test]
    fn test_gpio_controller() {
        let ctrl = GpioController::new(0x1000, 32);
        assert_eq!(ctrl.num_pins(), 32);

        let pin = ctrl.pin(5).unwrap();
        assert_eq!(pin.pin_number(), 5);

        assert!(ctrl.pin(32).is_err());
    }

    #[test]
    fn test_gpio_set_invalid() {
        let pin = GpioPin::new(0x1000, 5);
        // Input pin, can't set
        assert!(pin.set(GpioValue::High).is_err());
    }
}
