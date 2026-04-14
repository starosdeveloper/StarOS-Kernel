// SPDX-License-Identifier: GPL-2.0-only
/*
 * Resource management subsystem
 *
 * Ported from Linux kernel/resource.c
 */

pub mod core;
pub mod mmio;
pub mod irq;

pub use core::{
    Resource, ResourceConstraint, ResourceDesc, ResourceError, Result,
    IORESOURCE_BUS, IORESOURCE_BUSY, IORESOURCE_DMA, IORESOURCE_EXCLUSIVE,
    IORESOURCE_IO, IORESOURCE_IRQ, IORESOURCE_MEM, IORESOURCE_MUXED,
    IORESOURCE_SYSTEM_RAM,
    adjust_resource, allocate_resource, get_iomem_resource, get_ioport_resource,
    insert_resource, lookup_resource, release_resource, remove_resource, request_resource,
};

pub use mmio::{
    MmioRegion, MmioError, IoremapType,
    ioremap, ioremap_uc, ioremap_wc, ioremap_np, iounmap,
    devm_ioremap, devm_ioremap_uc, devm_ioremap_wc,
    devm_ioremap_resource, devm_ioremap_resource_wc,
    devm_platform_ioremap_resource,
};

pub use irq::{
    IrqHandler, IrqReturn, IrqError,
    IRQF_SHARED, IRQF_TRIGGER_RISING, IRQF_TRIGGER_FALLING,
    IRQF_TRIGGER_HIGH, IRQF_TRIGGER_LOW, IRQF_ONESHOT,
    request_irq, free_irq, devm_request_irq, devm_free_irq,
    enable_irq, disable_irq, handle_irq,
    irq_of_resource, platform_get_irq,
};
