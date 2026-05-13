#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz the device tree parser
    let _ = staros_kernel::devicetree::parser::FdtParser::new(data);
});
