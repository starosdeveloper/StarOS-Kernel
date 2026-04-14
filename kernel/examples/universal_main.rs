//! Example: Universal kernel main entry point
//!
//! This replaces device-specific initialization with universal auto-detection

#![no_std]
#![no_main]

use kernel::{
    universal_init::init_universal_kernel,
    device_probe::get_device_info,
    driver_registry::registry,
};

#[no_mangle]
pub extern "C" fn _start(dtb_addr: usize) -> ! {
    kernel_main(dtb_addr)
}

#[no_mangle]
pub extern "C" fn kernel_main(dtb_addr: usize) -> ! {
    // ============================================
    // UNIVERSAL KERNEL INITIALIZATION
    // ============================================
    
    // Step 1: Initialize universal kernel
    // This will:
    // - Parse Device Tree
    // - Register all built-in drivers
    // - Probe and initialize all devices
    match init_universal_kernel(dtb_addr) {
        Ok(()) => {
            // Success! All devices are initialized
        }
        Err(e) => {
            // Early panic - can't even initialize UART
            loop {}
        }
    }
    
    // Step 2: Get device information
    let device_info = match get_device_info(dtb_addr) {
        Ok(info) => info,
        Err(_) => loop {},
    };
    
    // Step 3: Print boot banner
    // UART is already initialized by universal_init
    print_banner(&device_info);
    
    // Step 4: Start system services
    start_system();
    
    // Main loop
    loop {
        // Handle interrupts, schedule tasks, etc.
    }
}

fn print_banner(info: &kernel::device_probe::DeviceInfo) {
    println!("╔════════════════════════════════════════╗");
    println!("║         STAR OS Universal Kernel       ║");
    println!("╚════════════════════════════════════════╝");
    println!();
    println!("Device: {}", info.model);
    println!("SoC: {:?}", info.soc);
    println!("Vendor: {}", info.soc.vendor());
    println!();
    
    // Print driver statistics
    let reg = registry();
    println!("Loaded {} drivers:", reg.count());
    for driver in reg.drivers().iter().flatten() {
        println!("  - {} (priority: {})", driver.name, driver.priority);
    }
    println!();
}

fn start_system() {
    println!("Starting system services...");
    
    // Initialize memory management
    // kernel::memory::init();
    
    // Initialize scheduler
    // kernel::scheduler::init();
    
    // Initialize IPC
    // kernel::ipc::init();
    
    // Start init process
    // kernel::task::spawn_init();
    
    println!("System ready!");
}

// Panic handler
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    loop {}
}

// Placeholder println macro (replace with actual UART implementation)
macro_rules! println {
    ($($arg:tt)*) => {{
        // uart::write_fmt(format_args!($($arg)*));
        // uart::write(b"\n");
    }};
}

use println;
