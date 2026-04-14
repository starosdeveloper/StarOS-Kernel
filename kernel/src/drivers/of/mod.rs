//! Open Firmware (Device Tree) Support
//! 
//! Ported from Linux kernel drivers/of/
//! 
//! This module provides Device Tree parsing and manipulation functionality.

pub mod fdt;
pub mod base;
pub mod property;
pub mod address;
pub mod irq;
pub mod platform;

// Re-export commonly used functions
pub use base::{
    DeviceNode, Property,
    of_property_read_u32, of_property_read_u64, of_property_read_string,
    of_device_is_available, of_device_is_compatible, of_get_property,
    of_get_parent, of_find_property,
};
pub use fdt::{of_fdt_unflatten_tree, of_fdt_device_is_available, FdtError, Result as FdtResult};
pub use crate::devicetree::parser::FdtParser;

/// Common OF error type (alias for FdtError)
pub type OfError = FdtError;
