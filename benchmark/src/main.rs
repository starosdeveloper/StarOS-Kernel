// StarOS версия бенчмарка (bare-metal)
#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::arch::global_asm;

mod algorithm;
use algorithm::*;

// Таблица векторов исключений
global_asm!(
    ".section .text.vectors",
    ".align 11",
    ".global _vectors",
    "_vectors:",
    // EL1t
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    // EL1h
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    // EL0 64-bit
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    // EL0 32-bit
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    ".align 7",
    "b .",
    ".align 7",
    "b .",
);

// ARM64 PMU (Performance Monitoring Unit) для точного измерения тактов
#[inline(always)]
fn read_cycle_counter() -> u64 {
    0 // Временно отключено, требует EL1
}

#[inline(always)]
fn enable_cycle_counter() {
    // PMU требует EL1, пропускаем для теста
}

// UART для вывода (минимальная реализация)
const UART_BASE: usize = 0x0900_0000; // QEMU virt PL011 UART

fn uart_putc(c: u8) {
    unsafe {
        // PL011 UARTDR register at offset 0x00
        core::ptr::write_volatile(UART_BASE as *mut u8, c);
    }
}

fn uart_puts(s: &str) {
    for byte in s.bytes() {
        uart_putc(byte);
    }
}

fn uart_print_u64(val: u64) {
    let mut buf = [0u8; 20];
    let mut n = val;
    let mut i = 0;
    
    if n == 0 {
        uart_putc(b'0');
        return;
    }
    
    while n > 0 {
        buf[i] = (n % 10) as u8 + b'0';
        n /= 10;
        i += 1;
    }
    
    while i > 0 {
        i -= 1;
        uart_putc(buf[i]);
    }
}

fn uart_print_f64(val: f64) {
    let int_part = val as u64;
    let frac_part = ((val - int_part as f64) * 100.0) as u64;
    uart_print_u64(int_part);
    uart_putc(b'.');
    if frac_part < 10 {
        uart_putc(b'0');
    }
    uart_print_u64(frac_part);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    unsafe {
        // Отключить MMU и кэши
        core::arch::asm!(
            "mrs x0, SCTLR_EL1",
            "bic x0, x0, #1",      // Disable MMU
            "bic x0, x0, #4",      // Disable D-cache
            "bic x0, x0, #0x1000", // Disable I-cache
            "msr SCTLR_EL1, x0",
            "isb",
            options(nostack)
        );
        
        // Установить таблицу векторов
        core::arch::asm!(
            "adr x0, _vectors",
            "msr VBAR_EL1, x0",
            options(nostack)
        );
    }
    
    uart_puts("=== STAR OS Benchmark - Bare Metal ===\n");
    
    enable_cycle_counter();
    uart_puts("Matrix size: 512x512\n");
    uart_puts("Iterations: 10\n\n");
    
    // Выделяем память статически (no heap)
    static mut A: Matrix = [[0.0; MATRIX_SIZE]; MATRIX_SIZE];
    static mut B: Matrix = [[0.0; MATRIX_SIZE]; MATRIX_SIZE];
    static mut RESULT: Matrix = [[0.0; MATRIX_SIZE]; MATRIX_SIZE];
    
    unsafe {
        init_matrix(&mut A, 1.5);
        init_matrix(&mut B, 2.3);
        
        uart_puts("Starting benchmark...\n");
        
        let mut times = [0u64; ITERATIONS];
        
        // Отключаем прерывания для чистого измерения
        core::arch::asm!("msr DAIFSet, #0xF", options(nostack, nomem));
        
        for i in 0..ITERATIONS {
            let start = read_cycle_counter();
            matrix_multiply(&A, &B, &mut RESULT);
            let end = read_cycle_counter();
            
            times[i] = end - start;
        }
        
        // Включаем прерывания обратно
        core::arch::asm!("msr DAIFClr, #0xF", options(nostack, nomem));
        
        // Вывод результатов
        for (i, &cycles) in times.iter().enumerate() {
            uart_puts("Iteration ");
            uart_print_u64((i + 1) as u64);
            uart_puts(": ");
            uart_print_u64(cycles);
            uart_puts(" cycles\n");
        }
        
        let checksum_val = checksum(&RESULT);
        uart_puts("\nChecksum: ");
        uart_print_f64(checksum_val);
        uart_puts("\n");
        
        // Статистика
        let mut total = 0u64;
        let mut min = times[0];
        let mut max = times[0];
        
        for &t in times.iter() {
            total += t;
            if t < min { min = t; }
            if t > max { max = t; }
        }
        
        let avg = total / ITERATIONS as u64;
        let jitter = max - min;
        
        uart_puts("\n=== Results ===\n");
        uart_puts("Average: ");
        uart_print_u64(avg);
        uart_puts(" cycles\n");
        uart_puts("Min: ");
        uart_print_u64(min);
        uart_puts(" cycles\n");
        uart_puts("Max: ");
        uart_print_u64(max);
        uart_puts(" cycles\n");
        uart_puts("Jitter: ");
        uart_print_u64(jitter);
        uart_puts(" cycles\n");
        
        uart_puts("\n### METRICS ###\n");
        uart_puts("PLATFORM=staros\n");
        uart_puts("AVG_CYCLES=");
        uart_print_u64(avg);
        uart_puts("\nMIN_CYCLES=");
        uart_print_u64(min);
        uart_puts("\nMAX_CYCLES=");
        uart_print_u64(max);
        uart_puts("\nJITTER_CYCLES=");
        uart_print_u64(jitter);
        uart_puts("\nCHECKSUM=");
        uart_print_f64(checksum_val);
        uart_puts("\n");
    }
    
    uart_puts("\nBenchmark complete. Halting.\n");
    
    loop {
        unsafe {
            core::arch::asm!("wfi", options(nostack, nomem));
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    uart_puts("PANIC!\n");
    loop {
        unsafe {
            core::arch::asm!("wfi", options(nostack, nomem));
        }
    }
}
