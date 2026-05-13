// SPDX-License-Identifier: MIT OR Apache-2.0
//! Interrupt-safe spinlocks for kernel use
//!
//! Linux-style spinlocks that disable interrupts during lock

use spin::Mutex as SpinMutex;
use core::ops::{Deref, DerefMut};

/// Interrupt-safe spinlock
pub struct IrqSafeMutex<T> {
    inner: SpinMutex<T>,
}

impl<T> IrqSafeMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            inner: SpinMutex::new(data),
        }
    }

    /// Lock with interrupts disabled (irqsave)
    pub fn lock_irqsave(&self) -> IrqSafeGuard<'_, T> {
        let flags = disable_interrupts();
        let guard = self.inner.lock();
        IrqSafeGuard { guard, flags }
    }

    /// Regular lock (for non-IRQ contexts)
    pub fn lock(&self) -> spin::MutexGuard<'_, T> {
        self.inner.lock()
    }
}

unsafe impl<T: Send> Send for IrqSafeMutex<T> {}
unsafe impl<T: Send> Sync for IrqSafeMutex<T> {}

/// Guard that restores interrupt state on drop
pub struct IrqSafeGuard<'a, T> {
    guard: spin::MutexGuard<'a, T>,
    flags: usize,
}

impl<'a, T> Drop for IrqSafeGuard<'a, T> {
    fn drop(&mut self) {
        restore_interrupts(self.flags);
    }
}

impl<'a, T> Deref for IrqSafeGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<'a, T> DerefMut for IrqSafeGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

#[inline]
fn disable_interrupts() -> usize {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let daif: usize;
        core::arch::asm!("mrs {}, DAIF", out(reg) daif);
        core::arch::asm!("msr DAIFSet, #0xF");
        daif
    }
    #[cfg(not(target_arch = "aarch64"))]
    0
}

#[inline]
fn restore_interrupts(flags: usize) {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("msr DAIF, {}", in(reg) flags);
    }
    #[cfg(not(target_arch = "aarch64"))]
    let _ = flags;
}
