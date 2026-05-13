use core::sync::atomic::{AtomicUsize, Ordering};

static EARLY_UART_BASE: AtomicUsize = AtomicUsize::new(0);

const UART_DR: usize = 0x00;
const UART_FR: usize = 0x18;
const UART_CR: usize = 0x30;

pub struct EarlyUart {
    base: usize,
}

impl EarlyUart {
    pub fn probe() -> Option<Self> {
        let addresses = [
            0x0A84000,  // Qualcomm (most common)
            0x11002000, // MediaTek
            0x13820000, // Exynos
            0x09000000, // QEMU virt
        ];

        for &addr in &addresses {
            if Self::try_init(addr) {
                EARLY_UART_BASE.store(addr, Ordering::Release);
                let uart = Self { base: addr };
                uart.puts("Early UART initialized at 0x");
                uart.print_hex(addr as u64);
                uart.puts("\n");
                return Some(uart);
            }
        }

        None
    }

    fn try_init(base: usize) -> bool {
        unsafe {
            let fr_addr = (base + UART_FR) as *const u32;
            let fr = core::ptr::read_volatile(fr_addr);
            
            if fr != 0xFFFFFFFF && fr != 0x00000000 {
                let cr_addr = (base + UART_CR) as *mut u32;
                core::ptr::write_volatile(cr_addr, 0x301);
                
                for _ in 0..100 {
                    core::hint::spin_loop();
                }
                
                return true;
            }
        }
        false
    }

    pub fn putc(&self, c: u8) {
        unsafe {
            let fr_addr = (self.base + UART_FR) as *const u32;
            for _ in 0..10000 {
                if (core::ptr::read_volatile(fr_addr) & 0x20) == 0 {
                    break;
                }
                core::hint::spin_loop();
            }

            let dr_addr = (self.base + UART_DR) as *mut u32;
            core::ptr::write_volatile(dr_addr, c as u32);
        }
    }

    pub fn puts(&self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.putc(b'\r');
            }
            self.putc(byte);
        }
    }

    pub fn print_hex(&self, val: u64) {
        for i in (0..16).rev() {
            let nibble = ((val >> (i * 4)) & 0xF) as u8;
            let c = if nibble < 10 {
                b'0' + nibble
            } else {
                b'A' + (nibble - 10)
            };
            self.putc(c);
        }
    }

    pub fn print_dec(&self, mut val: u32) {
        if val == 0 {
            self.putc(b'0');
            return;
        }

        let mut buf = [0u8; 10];
        let mut i = 0;

        while val > 0 {
            buf[i] = b'0' + (val % 10) as u8;
            val /= 10;
            i += 1;
        }

        while i > 0 {
            i -= 1;
            self.putc(buf[i]);
        }
    }

    pub fn base_addr(&self) -> usize {
        self.base
    }
}

pub fn early_debug_init() -> Option<EarlyUart> {
    EarlyUart::probe()
}

pub fn get_early_uart() -> Option<usize> {
    let base = EARLY_UART_BASE.load(Ordering::Acquire);
    if base != 0 {
        Some(base)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_early_uart_probe() {
        let _ = EarlyUart::probe();
    }

    #[test]
    fn test_early_debug_init() {
        let _ = early_debug_init();
    }

    #[test]
    fn test_get_early_uart() {
        let _ = get_early_uart();
    }
}
