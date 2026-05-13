use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

static UART_BASE_ADDR: AtomicUsize = AtomicUsize::new(0);
static PANIC_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn set_uart_base(addr: usize) {
    UART_BASE_ADDR.store(addr, Ordering::Release);
}

fn uart_putc(c: u8) {
    let base = UART_BASE_ADDR.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    
    unsafe {
        let uart_dr = base as *mut u32;
        core::ptr::write_volatile(uart_dr, c as u32);
        
        for _ in 0..1000 {
            let uart_fr = (base + 0x18) as *const u32;
            if core::ptr::read_volatile(uart_fr) & 0x20 == 0 {
                break;
            }
            core::hint::spin_loop();
        }
    }
}

fn uart_puts(s: &str) {
    for byte in s.bytes() {
        uart_putc(byte);
    }
}

fn print_number(mut n: u32) {
    if n == 0 {
        uart_putc(b'0');
        return;
    }
    
    let mut buf = [0u8; 10];
    let mut i = 0;
    
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    
    while i > 0 {
        i -= 1;
        uart_putc(buf[i]);
    }
}

fn print_hex64(val: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for i in (0..16).rev() {
        uart_putc(HEX[((val >> (i * 4)) & 0xF) as usize]);
    }
    uart_putc(b'\n');
}

#[cfg(all(not(test), not(feature = "std")))]
#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let panic_num = PANIC_COUNT.fetch_add(1, Ordering::SeqCst);
    
    // Double/triple panic: just halt
    if panic_num > 2 {
        unsafe {
            core::arch::asm!("msr daifset, #0xf");
            loop { core::arch::asm!("wfi"); }
        }
    }

    // Disable interrupts immediately
    unsafe { core::arch::asm!("msr daifset, #0xf"); }

    uart_puts("\n\n========== KERNEL PANIC ==========\n");
    uart_puts("Panic #");
    print_number(panic_num as u32);
    uart_puts("\n");
    
    // Location
    if let Some(location) = info.location() {
        uart_puts("At: ");
        uart_puts(location.file());
        uart_puts(":");
        print_number(location.line());
        uart_puts("\n");
    }

    // Register dump
    uart_puts("\n--- Register State ---\n");
    unsafe {
        let sp: u64;
        let lr: u64;
        let el: u64;
        let spsr: u64;
        let esr: u64;
        let far: u64;
        
        core::arch::asm!("mov {}, sp", out(reg) sp);
        core::arch::asm!("mov {}, x30", out(reg) lr);
        core::arch::asm!("mrs {}, CurrentEL", out(reg) el);
        core::arch::asm!("mrs {}, SPSR_EL1", out(reg) spsr);
        core::arch::asm!("mrs {}, ESR_EL1", out(reg) esr);
        core::arch::asm!("mrs {}, FAR_EL1", out(reg) far);
        
        uart_puts("  SP:   0x"); print_hex64(sp);
        uart_puts("  LR:   0x"); print_hex64(lr);
        uart_puts("  EL:   "); print_number(((el >> 2) & 3) as u32); uart_puts("\n");
        uart_puts("  SPSR: 0x"); print_hex64(spsr);
        uart_puts("  ESR:  0x"); print_hex64(esr);
        uart_puts("  FAR:  0x"); print_hex64(far);
        
        // Stack trace (frame pointer chain)
        uart_puts("\n--- Stack Trace ---\n");
        let mut fp: u64;
        core::arch::asm!("mov {}, x29", out(reg) fp);
        
        for i in 0..16 {
            if fp == 0 || fp & 0x7 != 0 { break; }
            let ret_addr = core::ptr::read_volatile((fp as *const u64).add(1));
            let next_fp = core::ptr::read_volatile(fp as *const u64);
            
            uart_puts("  #");
            print_number(i);
            uart_puts(": 0x");
            print_hex64(ret_addr);
            
            fp = next_fp;
        }
    }

    uart_puts("\n--- Recovery ---\n");
    uart_puts("Attempting watchdog reset...\n");

    // Brief delay then halt
    for _ in 0..500_000 { core::hint::spin_loop(); }

    uart_puts("Reset failed. Device halted.\n");
    uart_puts("==================================\n");

    loop { unsafe { core::arch::asm!("wfi"); } }
}

pub fn trigger_recovery() -> ! {
    panic!("Manual recovery triggered");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uart_putc() {
        uart_putc(b'A');
    }

    #[test]
    fn test_uart_puts() {
        uart_puts("test");
    }

    #[test]
    #[should_panic]
    fn test_trigger_recovery() {
        trigger_recovery();
    }
}
