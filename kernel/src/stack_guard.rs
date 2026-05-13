//! Stack canary support for -Z stack-protector=strong

use core::sync::atomic::{AtomicU64, Ordering};

static STACK_GUARD: AtomicU64 = AtomicU64::new(0);

/// Initialize stack guard with random value
pub fn init_stack_guard(entropy: u64) {
    let guard = entropy ^ 0xDEAD_BEEF_CAFE_BABEu64;
    let guard = guard.rotate_left(13) ^ guard.wrapping_mul(0x517cc1b727220a95);
    STACK_GUARD.store(guard, Ordering::Release);
}

pub fn get_stack_guard() -> u64 {
    STACK_GUARD.load(Ordering::Acquire)
}

/// Called when stack corruption detected. Must never return.
#[cfg(not(any(test, feature = "std")))] 
#[no_mangle]
pub extern "C" fn __stack_chk_fail() -> ! {
    #[cfg(target_arch = "aarch64")]
    unsafe { core::arch::asm!("msr daifset, #0xf", options(nostack)); }
    panic!("stack buffer overflow detected");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_init_guard() {
        init_stack_guard(0x1234);
        assert_ne!(get_stack_guard(), 0);
    }
}
