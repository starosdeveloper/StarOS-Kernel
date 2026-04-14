//! Modem driver for cellular connectivity
//! 
//! Supports Quectel EG25-G (PinePhone Pro) and generic AT modems.
//! 
//! Hardware details (PinePhone Pro):
//! - UART: /dev/ttyUSB2 (115200 baud)
//! - Power GPIO: GPIO4_C0 (pin 148)
//! - Reset GPIO: GPIO4_D2 (pin 154)
//! - Status GPIO: GPIO4_D3 (pin 155)

use core::ptr::{read_volatile, write_volatile};

/// Modem errors
#[derive(Debug)]
pub enum ModemError {
    NotInitialized,
    PowerFailed,
    Timeout,
    CommandFailed,
    UartError,
}

/// GPIO controller for RK3399
struct GPIO {
    base: usize,
}

impl GPIO {
    fn new(base: usize) -> Self {
        Self { base }
    }
    
    /// Set pin high
    fn set_high(&self, pin: u32) {
        unsafe {
            let reg = (self.base + 0x00) as *mut u32;
            let mut val = read_volatile(reg);
            val |= 1 << pin;
            write_volatile(reg, val);
        }
    }
    
    /// Set pin low
    fn set_low(&self, pin: u32) {
        unsafe {
            let reg = (self.base + 0x00) as *mut u32;
            let mut val = read_volatile(reg);
            val &= !(1 << pin);
            write_volatile(reg, val);
        }
    }
    
    /// Read pin state
    fn read(&self, pin: u32) -> bool {
        unsafe {
            let reg = (self.base + 0x50) as *mut u32;
            let val = read_volatile(reg);
            (val & (1 << pin)) != 0
        }
    }
}

/// UART controller
struct UART {
    base: usize,
}

impl UART {
    fn new(base: usize) -> Self {
        Self { base }
    }
    
    /// Initialize UART (115200 baud, 8N1)
    fn init(&self) {
        unsafe {
            // Disable UART
            write_volatile((self.base + 0x30) as *mut u32, 0);
            
            // Set baud rate (115200)
            write_volatile((self.base + 0x24) as *mut u32, 0x0D); // Divisor
            
            // 8N1, FIFO enabled
            write_volatile((self.base + 0x2C) as *mut u32, 0x03);
            
            // Enable UART
            write_volatile((self.base + 0x30) as *mut u32, 0x01);
        }
    }
    
    /// Send byte
    fn send_byte(&self, byte: u8) {
        unsafe {
            // Wait for TX ready
            while (read_volatile((self.base + 0x7C) as *mut u32) & 0x20) == 0 {}
            
            // Write byte
            write_volatile((self.base + 0x00) as *mut u32, byte as u32);
        }
    }
    
    /// Receive byte (non-blocking)
    fn recv_byte(&self) -> Option<u8> {
        unsafe {
            // Check if data available
            if (read_volatile((self.base + 0x7C) as *mut u32) & 0x01) != 0 {
                Some(read_volatile((self.base + 0x00) as *mut u32) as u8)
            } else {
                None
            }
        }
    }
    
    /// Send string
    fn send_str(&self, s: &str) {
        for byte in s.bytes() {
            self.send_byte(byte);
        }
        self.send_byte(b'\r');
        self.send_byte(b'\n');
    }
    
    /// Receive line (blocking with timeout)
    fn recv_line(&self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, ModemError> {
        let mut idx = 0;
        let mut iterations = timeout_ms * 1000; // ~1us per iteration
        
        while idx < buf.len() && iterations > 0 {
            if let Some(byte) = self.recv_byte() {
                if byte == b'\n' {
                    return Ok(idx);
                }
                if byte != b'\r' {
                    buf[idx] = byte;
                    idx += 1;
                }
            } else {
                iterations -= 1;
                // Busy wait ~1us
                for _ in 0..100 { core::hint::spin_loop(); }
            }
        }
        
        if iterations == 0 {
            Err(ModemError::Timeout)
        } else {
            Ok(idx)
        }
    }
}

/// Modem driver
pub struct ModemDriver {
    uart: UART,
    gpio: GPIO,
    power_pin: u32,
    reset_pin: u32,
    status_pin: u32,
    powered: bool,
    ready: bool,
}

static mut MODEM_DRIVER: Option<ModemDriver> = None;

impl ModemDriver {
    /// Create new modem driver
    pub fn new() -> Self {
        // PinePhone Pro addresses
        let uart_base = 0xFF180000; // UART2
        let gpio_base = 0xFF790000; // GPIO4
        
        let uart = UART::new(uart_base);
        uart.init();
        
        Self {
            uart,
            gpio: GPIO::new(gpio_base),
            power_pin: 16, // GPIO4_C0
            reset_pin: 26, // GPIO4_D2
            status_pin: 27, // GPIO4_D3
            powered: false,
            ready: false,
        }
    }
    
    /// Power on modem (1.5s pulse)
    pub fn power_on(&mut self) -> Result<(), ModemError> {
        // Set power pin high
        self.gpio.set_high(self.power_pin);
        
        // Wait 1.5s (1,500,000 us)
        for _ in 0..1_500_000 {
            for _ in 0..100 { core::hint::spin_loop(); }
        }
        
        // Set power pin low
        self.gpio.set_low(self.power_pin);
        
        // Wait for modem ready (check status pin)
        let mut timeout = 5000; // 5 seconds
        while timeout > 0 {
            if self.gpio.read(self.status_pin) {
                self.powered = true;
                self.ready = true;
                return Ok(());
            }
            
            // Wait 1ms
            for _ in 0..1000 {
                for _ in 0..100 { core::hint::spin_loop(); }
            }
            timeout -= 1;
        }
        
        Err(ModemError::Timeout)
    }
    
    /// Power off modem (3s pulse)
    pub fn power_off(&mut self) -> Result<(), ModemError> {
        self.gpio.set_high(self.power_pin);
        
        // Wait 3s
        for _ in 0..3_000_000 {
            for _ in 0..100 { core::hint::spin_loop(); }
        }
        
        self.gpio.set_low(self.power_pin);
        
        self.powered = false;
        self.ready = false;
        Ok(())
    }
    
    /// Reset modem
    pub fn reset(&mut self) -> Result<(), ModemError> {
        self.gpio.set_high(self.reset_pin);
        
        // Wait 200ms
        for _ in 0..200_000 {
            for _ in 0..100 { core::hint::spin_loop(); }
        }
        
        self.gpio.set_low(self.reset_pin);
        
        // Wait for ready
        self.ready = false;
        for _ in 0..5000 {
            if self.gpio.read(self.status_pin) {
                self.ready = true;
                return Ok(());
            }
            for _ in 0..1000 {
                for _ in 0..100 { core::hint::spin_loop(); }
            }
        }
        
        Err(ModemError::Timeout)
    }
    
    /// Send AT command and receive response
    pub fn send_at(&mut self, cmd: &str) -> Result<heapless::String<256>, ModemError> {
        if !self.ready {
            return Err(ModemError::NotInitialized);
        }
        
        // Send command
        self.uart.send_str(cmd);
        
        // Read response lines until "OK" or "ERROR"
        let mut response = heapless::String::<256>::new();
        let mut buf = [0u8; 128];
        
        loop {
            let len = self.uart.recv_line(&mut buf, 1000)?;
            let line = core::str::from_utf8(&buf[..len])
                .map_err(|_| ModemError::UartError)?;
            
            if line.is_empty() {
                continue;
            }
            
            if line == "OK" || line == "ERROR" {
                if !response.is_empty() {
                    response.push('\n').ok();
                }
                response.push_str(line).ok();
                break;
            }
            
            if !response.is_empty() {
                response.push('\n').ok();
            }
            response.push_str(line).ok();
        }
        
        Ok(response)
    }
    
    /// Check if modem is ready
    pub fn is_ready(&self) -> bool {
        self.ready && self.gpio.read(self.status_pin)
    }
    
    /// Send raw bytes (for SMS text + Ctrl+Z)
    pub fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), ModemError> {
        if !self.ready {
            return Err(ModemError::NotInitialized);
        }
        
        for &byte in bytes {
            self.uart.send_byte(byte);
        }
        
        Ok(())
    }
    
    /// Receive line non-blocking (for URC polling)
    pub fn recv_line_nonblock(&mut self, buf: &mut [u8]) -> Result<usize, ModemError> {
        if !self.ready {
            return Err(ModemError::NotInitialized);
        }
        
        let mut idx = 0;
        let mut attempts = 100; // Quick poll, don't wait long
        
        while idx < buf.len() && attempts > 0 {
            if let Some(byte) = self.uart.recv_byte() {
                if byte == b'\n' {
                    return Ok(idx);
                }
                if byte != b'\r' {
                    buf[idx] = byte;
                    idx += 1;
                }
            } else {
                attempts -= 1;
            }
        }
        
        Ok(idx)
    }
}

/// Initialize modem driver
pub fn init() -> Result<(), &'static str> {
    unsafe {
        MODEM_DRIVER = Some(ModemDriver::new());
    }
    Ok(())
}

/// Get modem driver instance
pub fn get_instance() -> Option<&'static mut ModemDriver> {
    unsafe { MODEM_DRIVER.as_mut() }
}
