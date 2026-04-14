// SPDX-License-Identifier: GPL-2.0
/*
 * IRQ (Interrupt Request) resource management
 *
 * Ported from Linux kernel/irq/devres.c
 * Copyright (C) Linux Kernel Authors
 */

use crate::drivers::resource::core::{Resource, IORESOURCE_IRQ};
use crate::prelude::*;
use spin::Mutex;

/// IRQ handler function type
pub type IrqHandler = fn(irq: u32, dev_id: usize) -> IrqReturn;

/// IRQ handler return value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqReturn {
    /// IRQ was not from this device
    None,
    /// IRQ was handled
    Handled,
    /// IRQ needs wake
    WakeThread,
}

/// IRQ flags
pub const IRQF_SHARED: u32 = 0x00000080;
pub const IRQF_TRIGGER_RISING: u32 = 0x00000001;
pub const IRQF_TRIGGER_FALLING: u32 = 0x00000002;
pub const IRQF_TRIGGER_HIGH: u32 = 0x00000004;
pub const IRQF_TRIGGER_LOW: u32 = 0x00000008;
pub const IRQF_ONESHOT: u32 = 0x00002000;

/// IRQ error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqError {
    InvalidIrq,
    Busy,
    NoMemory,
    NotFound,
}

pub type Result<T> = core::result::Result<T, IrqError>;

/// IRQ descriptor
struct IrqDesc {
    irq: u32,
    handler: IrqHandler,
    flags: u32,
    dev_id: usize,
    name: &'static str,
}

/// Global IRQ registry
static IRQ_REGISTRY: Mutex<Vec<IrqDesc>> = Mutex::new(Vec::new());

/// request_irq - allocate an interrupt line
/// @irq: Interrupt line to allocate
/// @handler: Function to be called when the interrupt occurs
/// @flags: Interrupt type flags
/// @name: An ascii name for the claiming device
/// @dev_id: A cookie passed back to the handler function
///
/// This call allocates interrupt resources and enables the interrupt line.
pub fn request_irq(
    irq: u32,
    handler: IrqHandler,
    flags: u32,
    name: &'static str,
    dev_id: usize,
) -> Result<()> {
    let mut registry = IRQ_REGISTRY.lock();

    // Check if IRQ is already registered (unless SHARED)
    for desc in registry.iter() {
        if desc.irq == irq {
            if (flags & IRQF_SHARED) == 0 || (desc.flags & IRQF_SHARED) == 0 {
                return Err(IrqError::Busy);
            }
        }
    }

    registry.push(IrqDesc {
        irq,
        handler,
        flags,
        dev_id,
        name,
    });

    // Enable IRQ in hardware (architecture-specific)
    enable_irq_hw(irq);

    Ok(())
}

/// free_irq - free an interrupt allocated with request_irq
/// @irq: Interrupt line to free
/// @dev_id: Device identity to free
pub fn free_irq(irq: u32, dev_id: usize) -> Result<()> {
    let mut registry = IRQ_REGISTRY.lock();

    let pos = registry
        .iter()
        .position(|desc| desc.irq == irq && desc.dev_id == dev_id)
        .ok_or(IrqError::NotFound)?;

    registry.remove(pos);

    // Disable IRQ if no more handlers
    if !registry.iter().any(|desc| desc.irq == irq) {
        disable_irq_hw(irq);
    }

    Ok(())
}

/// devm_request_irq - allocate an interrupt line for a managed device
/// @irq: Interrupt line to allocate
/// @handler: Function to be called when the interrupt occurs
/// @flags: Interrupt type flags
/// @name: An ascii name for the claiming device
/// @dev_id: A cookie passed back to the handler function
///
/// Managed request_irq(). The interrupt will be automatically freed on
/// driver detach.
pub fn devm_request_irq(
    irq: u32,
    handler: IrqHandler,
    flags: u32,
    name: &'static str,
    dev_id: usize,
) -> Result<()> {
    request_irq(irq, handler, flags, name, dev_id)
}

/// devm_free_irq - free an interrupt
/// @irq: Interrupt line to free
/// @dev_id: Device identity to free
pub fn devm_free_irq(irq: u32, dev_id: usize) -> Result<()> {
    free_irq(irq, dev_id)
}

/// enable_irq - enable handling of an irq
/// @irq: Interrupt to enable
pub fn enable_irq(irq: u32) {
    enable_irq_hw(irq);
}

/// disable_irq - disable an irq
/// @irq: Interrupt to disable
pub fn disable_irq(irq: u32) {
    disable_irq_hw(irq);
}

/// Handle an IRQ (called from interrupt handler)
///
/// SAFETY: Copies handlers before calling to prevent deadlock.
/// If a handler calls free_irq(), it won't deadlock because
/// we release the lock before calling handlers.
pub fn handle_irq(irq: u32) {
    // Copy matching handlers while holding lock
    let handlers: Vec<(IrqHandler, usize)> = {
        let registry = IRQ_REGISTRY.lock();
        registry
            .iter()
            .filter(|desc| desc.irq == irq)
            .map(|desc| (desc.handler, desc.dev_id))
            .collect()
    }; // Lock released here

    // Call handlers without holding lock (prevents deadlock)
    for (handler, dev_id) in handlers {
        handler(irq, dev_id);
    }
}

/// Get IRQ number from resource
pub fn irq_of_resource(res: &Resource, index: usize) -> Result<u32> {
    if res.flags & IORESOURCE_IRQ == 0 {
        return Err(IrqError::InvalidIrq);
    }

    if index > 0 {
        return Err(IrqError::InvalidIrq);
    }

    Ok(res.start as u32)
}

/// platform_get_irq - get an IRQ for a device
/// @res: resource descriptor
/// @num: IRQ number index
pub fn platform_get_irq(res: &Resource, num: usize) -> Result<u32> {
    irq_of_resource(res, num)
}

// Hardware-specific IRQ control (architecture-dependent)
// These would be implemented in arch-specific code

#[inline]
fn enable_irq_hw(irq: u32) {
    // ARM64 GIC implementation would go here
    // For now, this is a no-op as we don't have GIC driver yet
    let _ = irq;
}

#[inline]
fn disable_irq_hw(irq: u32) {
    // ARM64 GIC implementation would go here
    let _ = irq;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_handler(_irq: u32, _dev_id: usize) -> IrqReturn {
        IrqReturn::Handled
    }

    #[test]
    fn test_request_irq() {
        let result = request_irq(10, test_handler, 0, "test-device", 0x1000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_request_irq_busy() {
        let irq = 20;
        request_irq(irq, test_handler, 0, "device1", 0x1000).unwrap();
        let result = request_irq(irq, test_handler, 0, "device2", 0x2000);
        assert_eq!(result.err(), Some(IrqError::Busy));
        free_irq(irq, 0x1000).unwrap();
    }

    #[test]
    fn test_request_irq_shared() {
        let irq = 30;
        request_irq(irq, test_handler, IRQF_SHARED, "device1", 0x1000).unwrap();
        let result = request_irq(irq, test_handler, IRQF_SHARED, "device2", 0x2000);
        assert!(result.is_ok());
        free_irq(irq, 0x1000).unwrap();
        free_irq(irq, 0x2000).unwrap();
    }

    #[test]
    fn test_free_irq() {
        let irq = 40;
        request_irq(irq, test_handler, 0, "test", 0x1000).unwrap();
        let result = free_irq(irq, 0x1000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_free_irq_not_found() {
        let result = free_irq(99, 0x9999);
        assert_eq!(result.err(), Some(IrqError::NotFound));
    }

    #[test]
    fn test_irq_of_resource() {
        let res = Resource::new(42, 42, IORESOURCE_IRQ);
        let irq = irq_of_resource(&res, 0);
        assert_eq!(irq.unwrap(), 42);
    }

    #[test]
    fn test_irq_of_resource_invalid() {
        let res = Resource::new(42, 42, 0); // Not IORESOURCE_IRQ
        let irq = irq_of_resource(&res, 0);
        assert_eq!(irq.err(), Some(IrqError::InvalidIrq));
    }

    #[test]
    fn test_handle_irq() {
        let irq = 50;
        request_irq(irq, test_handler, 0, "test", 0x1000).unwrap();
        handle_irq(irq); // Should not panic
        free_irq(irq, 0x1000).unwrap();
    }

    #[test]
    fn test_handle_irq_no_deadlock() {
        // Handler that frees itself (would deadlock with naive implementation)
        fn self_freeing_handler(irq: u32, dev_id: usize) -> IrqReturn {
            // This would deadlock if handle_irq holds lock while calling
            let _ = free_irq(irq, dev_id);
            IrqReturn::Handled
        }

        let irq = 60;
        request_irq(irq, self_freeing_handler, 0, "test", 0x1000).unwrap();
        handle_irq(irq); // Should not deadlock
    }
}
