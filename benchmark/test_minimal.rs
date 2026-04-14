#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Простейший вывод в UART
    unsafe {
        let uart = 0x09000000 as *mut u8;
        let msg = b"Hello from STAR OS!\n";
        for &byte in msg {
            core::ptr::write_volatile(uart, byte);
        }
    }
    
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}
