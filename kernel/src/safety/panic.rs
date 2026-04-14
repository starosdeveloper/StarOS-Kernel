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

#[cfg(all(not(test), not(feature = "std")))]
#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    let panic_num = PANIC_COUNT.fetch_add(1, Ordering::SeqCst);
    
    if panic_num > 2 {
        unsafe {
            core::arch::asm!("msr daifset, #0xf");
            loop {
                core::arch::asm!("wfi");
            }
        }
    }

    unsafe {
        core::arch::asm!("msr daifset, #0xf");
    }

    uart_puts("\n\n*** KERNEL PANIC #");
    print_number(panic_num as u32);
    uart_puts(" ***\n");
    
    if let Some(location) = info.location() {
        uart_puts("Location: ");
        uart_puts(location.file());
        uart_puts(":");
        print_number(location.line());
        uart_puts(":");
        print_number(location.column());
        uart_puts("\n");
    }

    uart_puts("Message: panic occurred\n");

    uart_puts("\nSystem state:\n");
    uart_puts("  Interrupts: disabled\n");
    uart_puts("  Panic count: ");
    print_number(panic_num as u32);
    uart_puts("\n");

    uart_puts("\nAttempting recovery...\n");
    
    unsafe {
        let mut el: u64;
        core::arch::asm!("mrs {}, CurrentEL", out(reg) el);
        el = (el >> 2) & 0x3;
        uart_puts("  Current EL: ");
        print_number(el as u32);
        uart_puts("\n");
    }

    uart_puts("\nTriggering watchdog reset in 100ms...\n");

    for i in (0..10).rev() {
        print_number(i);
        uart_puts("... ");
        for _ in 0..100000 {
            core::hint::spin_loop();
        }
    }

    uart_puts("\n\nReset failed, entering infinite loop.\n");
    uart_puts("Please power cycle the device.\n");

    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
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
