#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(
    ".section .text.boot",
    ".global _start",
    "_start:",
    "mov x19, x0",
    "adr x0, __stack_top",
    "mov sp, x0",
    "adr x0, __bss_start",
    "adr x1, __bss_end",
    "1:",
    "cmp x0, x1",
    "b.ge 2f",
    "str xzr, [x0], #8",
    "b 1b",
    "2:",
    "mov x0, x19",
    "b kernel_main",
);

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

const UART_BASE: usize = 0x09000000;

fn uart_putc(c: u8) {
    unsafe {
        core::ptr::write_volatile(UART_BASE as *mut u8, c);
    }
}

fn uart_puts(s: &str) {
    for byte in s.bytes() {
        uart_putc(byte);
    }
}

#[no_mangle]
extern "C" fn kernel_main(_dtb_addr: usize) -> ! {
    uart_puts("\n=== STAR OS Kernel v0.1.0-alpha ===\n");
    uart_puts("QEMU Test: OK\n");
    uart_puts("UART: OK\n");
    uart_puts("\n=== Kernel Ready ===\n");
    
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
