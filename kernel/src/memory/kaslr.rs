//! Kernel Address Space Layout Randomization (KASLR)
//!
//! Randomizes the kernel base address at boot using hardware entropy.

use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};

/// Maximum KASLR offset: 512MB
const MAX_OFFSET: u64 = 512 * 1024 * 1024;
/// Page size for alignment: 4KB
const PAGE_SIZE: u64 = 4096;

static KASLR_OFFSET: AtomicU64 = AtomicU64::new(0);
static KASLR_ENABLED: AtomicBool = AtomicBool::new(false);

/// Initialize KASLR with an entropy source (e.g., ARM CNTVCT or hardware RNG).
/// Must be called once during early boot. Subsequent calls are ignored.
pub fn init_kaslr(entropy_source: u64) {
    if KASLR_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    // Mix entropy with a simple xorshift to spread bits
    let mixed = xorshift64(entropy_source);
    // Constrain to range and page-align
    let offset = (mixed % MAX_OFFSET) & !(PAGE_SIZE - 1);
    KASLR_OFFSET.store(offset, Ordering::Release);
    KASLR_ENABLED.store(true, Ordering::Release);
}

/// Returns the current KASLR offset (0 if not initialized).
pub fn kaslr_offset() -> u64 {
    KASLR_OFFSET.load(Ordering::Acquire)
}

/// Returns whether KASLR has been initialized.
pub fn is_enabled() -> bool {
    KASLR_ENABLED.load(Ordering::Acquire)
}

/// Applies the KASLR offset to a base address.
pub fn randomize_address(base: u64) -> u64 {
    base.wrapping_add(kaslr_offset())
}

/// Simple xorshift64 to mix entropy bits.
fn xorshift64(mut x: u64) -> u64 {
    if x == 0 {
        x = 0xdeadbeefcafe1234;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reset state for tests (not safe for concurrent use, test-only)
    fn reset() {
        KASLR_OFFSET.store(0, Ordering::SeqCst);
        KASLR_ENABLED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_not_enabled_initially() {
        reset();
        assert!(!is_enabled());
        assert_eq!(kaslr_offset(), 0);
    }

    #[test]
    fn test_init_enables_kaslr() {
        reset();
        init_kaslr(0xABCD_1234_5678_9ABC);
        assert!(is_enabled());
        assert_ne!(kaslr_offset(), 0);
    }

    #[test]
    fn test_offset_page_aligned() {
        reset();
        init_kaslr(0x1234_5678_DEAD_BEEF);
        assert_eq!(kaslr_offset() % PAGE_SIZE, 0);
    }

    #[test]
    fn test_offset_within_range() {
        reset();
        init_kaslr(0xFFFF_FFFF_FFFF_FFFF);
        assert!(kaslr_offset() < MAX_OFFSET);
    }

    #[test]
    fn test_init_idempotent() {
        reset();
        init_kaslr(111);
        let first = kaslr_offset();
        init_kaslr(999);
        assert_eq!(kaslr_offset(), first);
    }

    #[test]
    fn test_randomize_address() {
        reset();
        init_kaslr(42);
        let offset = kaslr_offset();
        assert_eq!(randomize_address(0x8000_0000), 0x8000_0000 + offset);
    }

    #[test]
    fn test_zero_entropy_handled() {
        reset();
        init_kaslr(0);
        assert!(is_enabled());
        assert_eq!(kaslr_offset() % PAGE_SIZE, 0);
        assert!(kaslr_offset() < MAX_OFFSET);
    }
}
