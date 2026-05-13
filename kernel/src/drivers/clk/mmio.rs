// SPDX-License-Identifier: MIT
//! MMIO helpers for clock registers with spinlock protection.
//!
//! Prevents Read-Modify-Write races when multiple clocks share the same register.

use core::sync::atomic::{AtomicBool, Ordering};

/// Global lock for clock MMIO operations.
/// Protects against concurrent RMW on shared registers.
static MMIO_LOCK: AtomicBool = AtomicBool::new(false);

/// Interrupt-safe lock guard.
struct IrqSafeLock {
    irq_enabled: bool,
}

impl IrqSafeLock {
    fn new() -> Self {
        // Disable interrupts and save state
        let irq_enabled = are_interrupts_enabled();
        disable_interrupts();
        
        // Spin until we acquire the lock
        while MMIO_LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        
        Self { irq_enabled }
    }
}

impl Drop for IrqSafeLock {
    fn drop(&mut self) {
        // Release lock
        MMIO_LOCK.store(false, Ordering::Release);
        
        // Restore interrupt state
        if self.irq_enabled {
            enable_interrupts();
        }
    }
}

#[inline]
fn are_interrupts_enabled() -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        let daif: u64;
        unsafe {
            core::arch::asm!("mrs {}, DAIF", out(reg) daif, options(nomem, nostack));
        }
        (daif & (1 << 7)) == 0
    }
    #[cfg(not(target_arch = "aarch64"))]
    { true }
}

#[inline]
fn disable_interrupts() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("msr DAIFSet, #2", options(nomem, nostack));
    }
}

#[inline]
fn enable_interrupts() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("msr DAIFClr, #2", options(nomem, nostack));
    }
}

/// Read a 32-bit register.
#[inline]
pub fn read_reg(addr: u64) -> u32 {
    unsafe { (addr as *const u32).read_volatile() }
}

/// Write a 32-bit register.
#[inline]
pub fn write_reg(addr: u64, val: u32) {
    unsafe { (addr as *mut u32).write_volatile(val) }
}

/// Atomic Read-Modify-Write: set bits in mask to corresponding bits in val.
///
/// ```ignore
/// // Set bit 3, clear bit 5
/// rmw_reg(0x1000, 0b101000, 0b001000);
/// ```
pub fn rmw_reg(addr: u64, mask: u32, val: u32) {
    let _lock = IrqSafeLock::new();
    let cur = read_reg(addr);
    write_reg(addr, (cur & !mask) | (val & mask));
}

/// Set specific bits (atomic).
pub fn set_bits(addr: u64, bits: u32) {
    let _lock = IrqSafeLock::new();
    let cur = read_reg(addr);
    write_reg(addr, cur | bits);
}

/// Clear specific bits (atomic).
pub fn clear_bits(addr: u64, bits: u32) {
    let _lock = IrqSafeLock::new();
    let cur = read_reg(addr);
    write_reg(addr, cur & !bits);
}
