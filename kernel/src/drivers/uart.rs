//! UART driver for serial console
//!
//! Supports PL011 (ARM) and 16550 (generic) UARTs

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::error::KernelError;

/// UART register offsets (PL011)
const UARTDR: usize = 0x00;     // Data register
const UARTFR: usize = 0x18;     // Flag register
const UARTIBRD: usize = 0x24;   // Integer baud rate
const UARTFBRD: usize = 0x28;   // Fractional baud rate
const UARTLCR_H: usize = 0x2C;  // Line control
const UARTCR: usize = 0x30;     // Control register
const UARTIMSC: usize = 0x38;   // Interrupt mask

/// UART flags
const FR_TXFF: u32 = 1 << 5;    // Transmit FIFO full
const FR_RXFE: u32 = 1 << 4;    // Receive FIFO empty

/// UART control bits
const CR_UARTEN: u32 = 1 << 0;  // UART enable
const CR_TXE: u32 = 1 << 8;     // Transmit enable
const CR_RXE: u32 = 1 << 9;     // Receive enable

/// Line control bits
const LCR_WLEN_8: u32 = 3 << 5; // 8-bit word length
const LCR_FEN: u32 = 1 << 4;    // FIFO enable

/// UART driver
pub struct Uart {
    base_addr: usize,
    initialized: AtomicBool,
    tx_bytes: AtomicU64,
    rx_bytes: AtomicU64,
}

impl Uart {
    pub const fn new(base_addr: usize) -> Self {
        Self {
            base_addr,
            initialized: AtomicBool::new(false),
            tx_bytes: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
        }
    }

    /// Initialize UART with given baud rate
    pub fn init(&self, baud_rate: u32, clock_freq: u32) -> Result<(), KernelError> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }

        // Disable UART
        self.write_reg(UARTCR, 0);

        // Calculate baud rate divisor
        // divisor = clock_freq / (16 * baud_rate)
        let divisor = (clock_freq * 4) / baud_rate;
        let ibrd = divisor >> 6;
        let fbrd = divisor & 0x3F;

        // Set baud rate
        self.write_reg(UARTIBRD, ibrd);
        self.write_reg(UARTFBRD, fbrd);

        // Set line control: 8N1, FIFO enabled
        self.write_reg(UARTLCR_H, LCR_WLEN_8 | LCR_FEN);

        // Enable UART, TX, RX
        self.write_reg(UARTCR, CR_UARTEN | CR_TXE | CR_RXE);

        self.initialized.store(true, Ordering::Release);
        Ok(())
    }

    /// Write a byte to UART
    pub fn write_byte(&self, byte: u8) {
        // Wait until TX FIFO not full
        while (self.read_reg(UARTFR) & FR_TXFF) != 0 {
            core::hint::spin_loop();
        }

        self.write_reg(UARTDR, byte as u32);
        self.tx_bytes.fetch_add(1, Ordering::Relaxed);
    }

    /// Read a byte from UART (blocking)
    pub fn read_byte(&self) -> u8 {
        // Wait until RX FIFO not empty
        while (self.read_reg(UARTFR) & FR_RXFE) != 0 {
            core::hint::spin_loop();
        }

        let byte = self.read_reg(UARTDR) as u8;
        self.rx_bytes.fetch_add(1, Ordering::Relaxed);
        byte
    }

    /// Try to read a byte (non-blocking)
    pub fn try_read_byte(&self) -> Option<u8> {
        if (self.read_reg(UARTFR) & FR_RXFE) != 0 {
            None
        } else {
            let byte = self.read_reg(UARTDR) as u8;
            self.rx_bytes.fetch_add(1, Ordering::Relaxed);
            Some(byte)
        }
    }

    /// Write a string to UART
    pub fn write_str(&self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
    }

    /// Write bytes to UART
    pub fn write(&self, buf: &[u8]) -> usize {
        for &byte in buf {
            self.write_byte(byte);
        }
        buf.len()
    }

    /// Read bytes from UART
    pub fn read(&self, buf: &mut [u8]) -> usize {
        let mut count = 0;
        for byte in buf.iter_mut() {
            if let Some(b) = self.try_read_byte() {
                *byte = b;
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Check if UART is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    /// Get statistics
    pub fn tx_bytes(&self) -> u64 {
        self.tx_bytes.load(Ordering::Relaxed)
    }

    pub fn rx_bytes(&self) -> u64 {
        self.rx_bytes.load(Ordering::Relaxed)
    }

    // Low-level register access
    #[inline]
    fn read_reg(&self, offset: usize) -> u32 {
        #[cfg(not(feature = "std"))]
        unsafe {
            core::ptr::read_volatile((self.base_addr + offset) as *const u32)
        }

        #[cfg(feature = "std")]
        0 // Stub for testing
    }

    #[inline]
    fn write_reg(&self, offset: usize, value: u32) {
        #[cfg(not(feature = "std"))]
        unsafe {
            core::ptr::write_volatile((self.base_addr + offset) as *mut u32, value);
        }

        #[cfg(feature = "std")]
        let _ = (offset, value); // Stub for testing
    }
}

unsafe impl Send for Uart {}
unsafe impl Sync for Uart {}

// Implement core::fmt::Write for easy printing
impl core::fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        Uart::write_str(self, s);
        Ok(())
    }
}

/// Global UART instance (will be initialized during boot)
pub static UART0: Uart = Uart::new(0x0900_0000); // Common QEMU address

/// Print macro using UART
#[macro_export]
macro_rules! uart_print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let uart = unsafe { &mut *(&$crate::drivers::uart::UART0 as *const _ as *mut $crate::drivers::Uart) };
        let _ = write!(uart, $($arg)*);
    }};
}

#[macro_export]
macro_rules! uart_println {
    () => { $crate::uart_print!("\n") };
    ($($arg:tt)*) => {{
        $crate::uart_print!($($arg)*);
        $crate::uart_print!("\n");
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uart_creation() {
        let uart = Uart::new(0x1000);
        assert!(!uart.is_initialized());
        assert_eq!(uart.tx_bytes(), 0);
        assert_eq!(uart.rx_bytes(), 0);
    }

    #[test]
    fn test_uart_init() {
        let uart = Uart::new(0x1000);
        // In std mode, this is a no-op
        assert!(uart.init(115200, 24_000_000).is_ok());
    }

    #[test]
    fn test_uart_write_str() {
        let uart = Uart::new(0x1000);
        uart.write_str("Hello, World!");
        // In std mode, this doesn't actually write
    }

    #[test]
    fn test_uart_statistics() {
        let uart = Uart::new(0x1000);
        
        #[cfg(feature = "std")]
        {
            // Manually increment for testing
            uart.tx_bytes.store(100, Ordering::Relaxed);
            assert_eq!(uart.tx_bytes(), 100);
        }
    }
}
