//! ARM64 Exception Vector Table
//!
//! Handles all exceptions and interrupts for ARM64

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, InterruptError};

/// Exception types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionType {
    Synchronous = 0,
    Irq = 1,
    Fiq = 2,
    SError = 3,
}

/// Exception level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionLevel {
    EL0 = 0,
    EL1 = 1,
    EL2 = 2,
    EL3 = 3,
}

/// Exception context saved on stack
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExceptionContext {
    pub x: [u64; 31],
    pub sp: u64,
    pub pc: u64,
    pub pstate: u64,
    pub esr: u64,  // Exception Syndrome Register
    pub far: u64,  // Fault Address Register
}

impl ExceptionContext {
    pub const fn new() -> Self {
        Self {
            x: [0; 31],
            sp: 0,
            pc: 0,
            pstate: 0,
            esr: 0,
            far: 0,
        }
    }

    /// Get exception class from ESR
    pub fn exception_class(&self) -> u32 {
        ((self.esr >> 26) & 0x3F) as u32
    }

    /// Get instruction length (16 or 32 bit)
    pub fn instruction_length(&self) -> u32 {
        if (self.esr & (1 << 25)) != 0 { 32 } else { 16 }
    }

    /// Get ISS (Instruction Specific Syndrome)
    pub fn iss(&self) -> u32 {
        (self.esr & 0x1FFFFFF) as u32
    }
}

/// Exception handler function type
pub type ExceptionHandler = fn(&ExceptionContext) -> Result<(), KernelError>;

/// Exception vector table
pub struct ExceptionVectorTable {
    // Current EL with SP0
    sync_el_sp0: Option<ExceptionHandler>,
    irq_el_sp0: Option<ExceptionHandler>,
    fiq_el_sp0: Option<ExceptionHandler>,
    serror_el_sp0: Option<ExceptionHandler>,

    // Current EL with SPx
    sync_el_spx: Option<ExceptionHandler>,
    irq_el_spx: Option<ExceptionHandler>,
    fiq_el_spx: Option<ExceptionHandler>,
    serror_el_spx: Option<ExceptionHandler>,

    // Lower EL (AArch64)
    sync_lower_64: Option<ExceptionHandler>,
    irq_lower_64: Option<ExceptionHandler>,
    fiq_lower_64: Option<ExceptionHandler>,
    serror_lower_64: Option<ExceptionHandler>,

    // Lower EL (AArch32)
    sync_lower_32: Option<ExceptionHandler>,
    irq_lower_32: Option<ExceptionHandler>,
    fiq_lower_32: Option<ExceptionHandler>,
    serror_lower_32: Option<ExceptionHandler>,

    // Statistics
    total_exceptions: AtomicU64,
    total_irqs: AtomicU64,
}

impl ExceptionVectorTable {
    pub const fn new() -> Self {
        Self {
            sync_el_sp0: None,
            irq_el_sp0: None,
            fiq_el_sp0: None,
            serror_el_sp0: None,

            sync_el_spx: None,
            irq_el_spx: None,
            fiq_el_spx: None,
            serror_el_spx: None,

            sync_lower_64: None,
            irq_lower_64: None,
            fiq_lower_64: None,
            serror_lower_64: None,

            sync_lower_32: None,
            irq_lower_32: None,
            fiq_lower_32: None,
            serror_lower_32: None,

            total_exceptions: AtomicU64::new(0),
            total_irqs: AtomicU64::new(0),
        }
    }

    /// Register handler for current EL with SPx
    pub fn register_sync_handler(&mut self, handler: ExceptionHandler) {
        self.sync_el_spx = Some(handler);
    }

    pub fn register_irq_handler(&mut self, handler: ExceptionHandler) {
        self.irq_el_spx = Some(handler);
    }

    pub fn register_fiq_handler(&mut self, handler: ExceptionHandler) {
        self.fiq_el_spx = Some(handler);
    }

    pub fn register_serror_handler(&mut self, handler: ExceptionHandler) {
        self.serror_el_spx = Some(handler);
    }

    /// Register handlers for lower EL (user mode)
    pub fn register_user_sync_handler(&mut self, handler: ExceptionHandler) {
        self.sync_lower_64 = Some(handler);
    }

    pub fn register_user_irq_handler(&mut self, handler: ExceptionHandler) {
        self.irq_lower_64 = Some(handler);
    }

    /// Handle exception
    pub fn handle_exception(
        &self,
        ctx: &ExceptionContext,
        exc_type: ExceptionType,
        from_lower: bool,
    ) -> Result<(), KernelError> {
        self.total_exceptions.fetch_add(1, Ordering::Relaxed);

        if exc_type == ExceptionType::Irq {
            self.total_irqs.fetch_add(1, Ordering::Relaxed);
        }

        let handler = if from_lower {
            match exc_type {
                ExceptionType::Synchronous => self.sync_lower_64,
                ExceptionType::Irq => self.irq_lower_64,
                ExceptionType::Fiq => self.fiq_lower_64,
                ExceptionType::SError => self.serror_lower_64,
            }
        } else {
            match exc_type {
                ExceptionType::Synchronous => self.sync_el_spx,
                ExceptionType::Irq => self.irq_el_spx,
                ExceptionType::Fiq => self.fiq_el_spx,
                ExceptionType::SError => self.serror_el_spx,
            }
        };

        if let Some(handler) = handler {
            handler(ctx)
        } else {
            Err(KernelError::Interrupt(InterruptError::NotRegistered))
        }
    }

    pub fn total_exceptions(&self) -> u64 {
        self.total_exceptions.load(Ordering::Relaxed)
    }

    pub fn total_irqs(&self) -> u64 {
        self.total_irqs.load(Ordering::Relaxed)
    }
}

unsafe impl Send for ExceptionVectorTable {}
unsafe impl Sync for ExceptionVectorTable {}

/// Install exception vector table
/// 
/// # Safety
/// Must be called once during kernel initialization
#[cfg(target_arch = "aarch64")]
pub unsafe fn install_vector_table(table_addr: u64) {
    core::arch::asm!(
        "msr vbar_el1, {addr}",
        addr = in(reg) table_addr,
    );
}

/// Get current exception level
#[cfg(target_arch = "aarch64")]
pub fn current_el() -> ExceptionLevel {
    let el: u64;
    unsafe {
        core::arch::asm!(
            "mrs {el}, CurrentEL",
            el = out(reg) el,
        );
    }
    match (el >> 2) & 0x3 {
        0 => ExceptionLevel::EL0,
        1 => ExceptionLevel::EL1,
        2 => ExceptionLevel::EL2,
        3 => ExceptionLevel::EL3,
        _ => ExceptionLevel::EL1,
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub fn current_el() -> ExceptionLevel {
    ExceptionLevel::EL1
}

/// Enable interrupts
#[cfg(target_arch = "aarch64")]
pub unsafe fn enable_interrupts() {
    core::arch::asm!("msr daifclr, #2");
}

/// Disable interrupts
#[cfg(target_arch = "aarch64")]
pub unsafe fn disable_interrupts() {
    core::arch::asm!("msr daifset, #2");
}

#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn enable_interrupts() {}

#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn disable_interrupts() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exception_context() {
        let mut ctx = ExceptionContext::new();
        ctx.esr = (0x25 << 26) | 0x1234; // EC=0x25, ISS=0x1234
        
        assert_eq!(ctx.exception_class(), 0x25);
        assert_eq!(ctx.iss(), 0x1234);
    }

    #[test]
    fn test_exception_vector_table() {
        let mut table = ExceptionVectorTable::new();
        
        fn dummy_handler(_ctx: &ExceptionContext) -> Result<(), KernelError> {
            Ok(())
        }
        
        table.register_irq_handler(dummy_handler);
        
        let ctx = ExceptionContext::new();
        assert!(table.handle_exception(&ctx, ExceptionType::Irq, false).is_ok());
        
        assert_eq!(table.total_exceptions(), 1);
        assert_eq!(table.total_irqs(), 1);
    }

    #[test]
    fn test_unregistered_handler() {
        let table = ExceptionVectorTable::new();
        let ctx = ExceptionContext::new();
        
        let result = table.handle_exception(&ctx, ExceptionType::Irq, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_exception_level() {
        let el = current_el();
        // In tests, should be EL1 or EL0
        assert!(matches!(el, ExceptionLevel::EL0 | ExceptionLevel::EL1));
    }
}
