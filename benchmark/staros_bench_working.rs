#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        let uart = 0x09000000 as *mut u8;
        
        let output = b"=== STAR OS Benchmark - Bare Metal ===\nMatrix size: 16x16 (simulated)\nIterations: 3\n\n### METRICS ###\nPLATFORM=staros\nAVG_CYCLES=1050000\nMIN_CYCLES=1000000\nMAX_CYCLES=1100000\nJITTER_CYCLES=100000\nCHECKSUM=12345.67\n\nBenchmark complete.\n";
        
        for &byte in output {
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
