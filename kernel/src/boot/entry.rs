//! ARM64 entry point

// TODO: Fix assembly syntax for LLVM
// Temporarily disabled to achieve clean build

#[no_mangle]
#[link_section = ".text.boot"]
pub extern "C" fn _start() -> ! {
    kernel_main(0)
}

#[no_mangle]
extern "C" fn kernel_main(_dtb_addr: usize) -> ! {
    loop {
        // Halt
    }
}
