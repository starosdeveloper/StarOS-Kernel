//! ARM GICv3 Interrupt Controller
//!
//! Generic Interrupt Controller version 3

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::error::{KernelError, InterruptError};

/// IRQ number type
pub type Irq = u32;

/// Interrupt handler
pub type IrqHandler = fn(Irq) -> Result<(), KernelError>;

/// Interrupt priority (0 = highest, 255 = lowest)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IrqPriority(u8);

impl IrqPriority {
    pub const HIGHEST: Self = Self(0);
    pub const HIGH: Self = Self(64);
    pub const NORMAL: Self = Self(128);
    pub const LOW: Self = Self(192);
    pub const LOWEST: Self = Self(255);

    pub const fn new(priority: u8) -> Self {
        Self(priority)
    }

    pub const fn as_u8(&self) -> u8 {
        self.0
    }
}

/// Interrupt configuration
#[derive(Clone, Copy)]
pub struct IrqConfig {
    pub priority: IrqPriority,
    pub enabled: bool,
    pub edge_triggered: bool,
    pub cpu_mask: u8, // CPU affinity mask
}

impl IrqConfig {
    pub const fn new(priority: IrqPriority) -> Self {
        Self {
            priority,
            enabled: false,
            edge_triggered: false,
            cpu_mask: 0xFF, // All CPUs
        }
    }
}

/// IRQ storm threshold - disable IRQ after this many spurious triggers
const SPURIOUS_THRESHOLD: u64 = 100;

/// GICv3 Interrupt Controller
pub struct InterruptController {
    handlers: [Option<IrqHandler>; 256],
    configs: [IrqConfig; 256],
    enabled: [AtomicBool; 256],
    unhandled_count: [AtomicU64; 256],
    total_irqs: AtomicU64,
    spurious_irqs: AtomicU64,
}

impl InterruptController {
    pub const fn new() -> Self {
        const INIT_HANDLER: Option<IrqHandler> = None;
        const INIT_CONFIG: IrqConfig = IrqConfig::new(IrqPriority::NORMAL);
        const INIT_ENABLED: AtomicBool = AtomicBool::new(false);
        const INIT_COUNT: AtomicU64 = AtomicU64::new(0);

        Self {
            handlers: [INIT_HANDLER; 256],
            configs: [INIT_CONFIG; 256],
            enabled: [INIT_ENABLED; 256],
            unhandled_count: [INIT_COUNT; 256],
            total_irqs: AtomicU64::new(0),
            spurious_irqs: AtomicU64::new(0),
        }
    }

    /// Register interrupt handler
    pub fn register(&mut self, irq: Irq, handler: IrqHandler, config: IrqConfig) -> Result<(), KernelError> {
        if irq >= 256 {
            return Err(KernelError::Interrupt(InterruptError::InvalidIrq));
        }

        let idx = irq as usize;

        // Reject if IRQ is currently active (enabled) - must unregister first
        if self.enabled[idx].load(Ordering::Acquire) {
            return Err(KernelError::Interrupt(InterruptError::AlreadyRegistered));
        }
        
        if self.handlers[idx].is_some() {
            return Err(KernelError::Interrupt(InterruptError::AlreadyRegistered));
        }

        self.handlers[idx] = Some(handler);
        self.configs[idx] = config;
        self.unhandled_count[idx].store(0, Ordering::Relaxed);

        Ok(())
    }

    /// Unregister interrupt handler
    pub fn unregister(&mut self, irq: Irq) -> Result<(), KernelError> {
        if irq >= 256 {
            return Err(KernelError::Interrupt(InterruptError::InvalidIrq));
        }

        let idx = irq as usize;
        
        if self.handlers[idx].is_none() {
            return Err(KernelError::Interrupt(InterruptError::NotRegistered));
        }

        // Disable before unregistering
        self.disable(irq)?;
        
        self.handlers[idx] = None;

        Ok(())
    }

    /// Enable interrupt
    pub fn enable(&self, irq: Irq) -> Result<(), KernelError> {
        if irq >= 256 {
            return Err(KernelError::Interrupt(InterruptError::InvalidIrq));
        }

        let idx = irq as usize;
        
        if self.handlers[idx].is_none() {
            return Err(KernelError::Interrupt(InterruptError::NotRegistered));
        }

        self.enabled[idx].store(true, Ordering::Release);

        // TODO: Write to GIC registers
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // Enable interrupt in GIC distributor
            // This is a placeholder - real implementation would write to MMIO
        }

        Ok(())
    }

    /// Disable interrupt
    pub fn disable(&self, irq: Irq) -> Result<(), KernelError> {
        if irq >= 256 {
            return Err(KernelError::Interrupt(InterruptError::InvalidIrq));
        }

        self.enabled[irq as usize].store(false, Ordering::Release);

        // TODO: Write to GIC registers
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // Disable interrupt in GIC distributor
        }

        Ok(())
    }

    /// Check if interrupt is enabled
    pub fn is_enabled(&self, irq: Irq) -> bool {
        if irq >= 256 {
            return false;
        }
        self.enabled[irq as usize].load(Ordering::Acquire)
    }

    /// Handle interrupt
    pub fn handle(&self, irq: Irq) -> Result<(), KernelError> {
        if irq >= 256 {
            self.spurious_irqs.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::Interrupt(InterruptError::InvalidIrq));
        }

        let idx = irq as usize;

        if !self.enabled[idx].load(Ordering::Acquire) {
            self.spurious_irqs.fetch_add(1, Ordering::Relaxed);
            return Err(KernelError::Interrupt(InterruptError::NotRegistered));
        }

        self.total_irqs.fetch_add(1, Ordering::Relaxed);

        if let Some(handler) = self.handlers[idx] {
            handler(irq)?;
        } else {
            // No registered handler - spurious IRQ storm detection
            let count = self.unhandled_count[idx].fetch_add(1, Ordering::Relaxed) + 1;
            self.spurious_irqs.fetch_add(1, Ordering::Relaxed);
            if count >= SPURIOUS_THRESHOLD {
                self.enabled[idx].store(false, Ordering::Release);
            }
            return Err(KernelError::Interrupt(InterruptError::NotRegistered));
        }

        // Send EOI (End Of Interrupt)
        self.send_eoi(irq);

        Ok(())
    }

    /// Send End Of Interrupt
    fn send_eoi(&self, irq: Irq) {
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // Write to ICC_EOIR1_EL1
            core::arch::asm!(
                "msr ICC_EOIR1_EL1, {irq}",
                irq = in(reg) irq as u64,
            );
        }

        #[cfg(not(target_arch = "aarch64"))]
        let _ = irq;
    }

    /// Set interrupt priority
    pub fn set_priority(&mut self, irq: Irq, priority: IrqPriority) -> Result<(), KernelError> {
        if irq >= 256 {
            return Err(KernelError::Interrupt(InterruptError::InvalidIrq));
        }

        self.configs[irq as usize].priority = priority;

        // TODO: Write to GIC registers

        Ok(())
    }

    /// Get interrupt priority
    pub fn get_priority(&self, irq: Irq) -> Option<IrqPriority> {
        if irq >= 256 {
            return None;
        }
        Some(self.configs[irq as usize].priority)
    }

    /// Set CPU affinity
    pub fn set_affinity(&mut self, irq: Irq, cpu_mask: u8) -> Result<(), KernelError> {
        if irq >= 256 {
            return Err(KernelError::Interrupt(InterruptError::InvalidIrq));
        }

        self.configs[irq as usize].cpu_mask = cpu_mask;

        // TODO: Write to GIC registers

        Ok(())
    }

    /// Get statistics
    pub fn total_irqs(&self) -> u64 {
        self.total_irqs.load(Ordering::Relaxed)
    }

    pub fn spurious_irqs(&self) -> u64 {
        self.spurious_irqs.load(Ordering::Relaxed)
    }
}

unsafe impl Send for InterruptController {}
unsafe impl Sync for InterruptController {}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_handler(_irq: Irq) -> Result<(), KernelError> {
        Ok(())
    }

    #[test]
    fn test_register_unregister() {
        let mut ctrl = InterruptController::new();
        let config = IrqConfig::new(IrqPriority::NORMAL);

        assert!(ctrl.register(10, dummy_handler, config).is_ok());
        assert!(ctrl.register(10, dummy_handler, config).is_err()); // Already registered

        assert!(ctrl.unregister(10).is_ok());
        assert!(ctrl.unregister(10).is_err()); // Not registered
    }

    #[test]
    fn test_enable_disable() {
        let mut ctrl = InterruptController::new();
        let config = IrqConfig::new(IrqPriority::NORMAL);

        ctrl.register(10, dummy_handler, config).unwrap();

        assert!(!ctrl.is_enabled(10));

        ctrl.enable(10).unwrap();
        assert!(ctrl.is_enabled(10));

        ctrl.disable(10).unwrap();
        assert!(!ctrl.is_enabled(10));
    }

    #[test]
    fn test_handle_irq() {
        let mut ctrl = InterruptController::new();
        let config = IrqConfig::new(IrqPriority::NORMAL);

        ctrl.register(10, dummy_handler, config).unwrap();
        ctrl.enable(10).unwrap();

        assert!(ctrl.handle(10).is_ok());
        assert_eq!(ctrl.total_irqs(), 1);
    }

    #[test]
    fn test_spurious_irq() {
        let ctrl = InterruptController::new();

        // Handle unregistered IRQ
        assert!(ctrl.handle(10).is_err());
        assert_eq!(ctrl.spurious_irqs(), 1);
    }

    #[test]
    fn test_priority() {
        let mut ctrl = InterruptController::new();

        ctrl.set_priority(10, IrqPriority::HIGH).unwrap();
        assert_eq!(ctrl.get_priority(10), Some(IrqPriority::HIGH));
    }

    #[test]
    fn test_invalid_irq() {
        let mut ctrl = InterruptController::new();
        let config = IrqConfig::new(IrqPriority::NORMAL);

        assert!(ctrl.register(256, dummy_handler, config).is_err());
        assert!(ctrl.enable(256).is_err());
        assert!(ctrl.handle(256).is_err());
    }
}
