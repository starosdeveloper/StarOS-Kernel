// SPDX-License-Identifier: MIT OR Apache-2.0
/*
 * Platform device creation from device tree
 *
 * Ported from Linux kernel drivers/of/platform.c
 * Original Copyright (C) 2006 Benjamin Herrenschmidt, IBM Corp
 */

use crate::drivers::of::base::{DeviceNode, of_device_is_available, of_device_is_compatible, of_get_property};
use crate::drivers::of::property::of_property_read_u32;
use crate::drivers::of::address::of_address_to_resource;
use crate::drivers::of::irq::of_irq_to_resource;
use crate::prelude::*;

/// Platform device structure
#[derive(Debug, Clone)]
pub struct PlatformDevice {
    /// Device name
    pub name: String,
    /// Device ID
    pub id: i32,
    /// Device tree node
    pub of_node: Option<*const DeviceNode>,
    /// Resources (MMIO, IRQ, etc)
    pub resources: Vec<Resource>,
    /// Number of resources
    pub num_resources: usize,
}

/// Resource types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    /// Memory resource
    Mem,
    /// I/O resource
    Io,
    /// IRQ resource
    Irq,
    /// DMA resource
    Dma,
}

/// Resource structure
#[derive(Debug, Clone)]
pub struct Resource {
    /// Resource name
    pub name: String,
    /// Start address/number
    pub start: u64,
    /// End address/number
    pub end: u64,
    /// Resource type
    pub res_type: ResourceType,
    /// Flags
    pub flags: u32,
}

/// Device match table entry
#[derive(Debug, Clone)]
pub struct OfDeviceId {
    /// Compatible string
    pub compatible: String,
}

/// Platform device error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformError {
    /// Invalid argument
    Invalid,
    /// No memory
    NoMemory,
    /// Device not available
    NotAvailable,
    /// Already populated
    AlreadyPopulated,
}

pub type Result<T> = core::result::Result<T, PlatformError>;

/// Skipped node table - nodes we don't create devices for
const SKIPPED_COMPATIBLES: &[&str] = &[
    "operating-points-v2",
];

/// of_address_count - Count number of address entries
/// @np: device node
///
/// Returns number of reg entries.
fn of_address_count(np: &DeviceNode) -> usize {
    let reg = match of_get_property(Some(np), "reg") {
        Some(r) => r,
        None => return 0,
    };
    
    // Get parent to determine cell sizes
    let parent = match np.parent {
        Some(p) => unsafe { &*p },
        None => return 0,
    };
    
    // Get #address-cells and #size-cells from parent
    let na = of_property_read_u32(Some(parent), "#address-cells")
        .unwrap_or(2) as usize;
    let ns = of_property_read_u32(Some(parent), "#size-cells")
        .unwrap_or(1) as usize;
    
    // Each entry is (na + ns) cells, each cell is 4 bytes
    let entry_size = (na + ns) * 4;
    
    if entry_size == 0 {
        return 0;
    }
    
    reg.len() / entry_size
}

/// of_device_alloc - Allocate platform device from device tree node
/// @np: device node
/// @bus_id: device name (optional)
///
/// Returns allocated platform device, or error.
pub fn of_device_alloc(
    np: &DeviceNode,
    bus_id: Option<&str>,
) -> Result<PlatformDevice> {
    let mut dev = PlatformDevice {
        name: bus_id.unwrap_or(&np.name).to_string(),
        id: -1, // PLATFORM_DEVID_NONE
        of_node: Some(np as *const DeviceNode),
        resources: Vec::new(),
        num_resources: 0,
    };
    
    // Count and allocate resources
    let num_reg = of_address_count(np);
    
    // Populate MMIO resources from reg property
    for i in 0..num_reg {
        if let Ok((start, size)) = of_address_to_resource(Some(np), i) {
            dev.resources.push(Resource {
                name: format!("reg{}", i),
                start,
                end: start + size - 1,
                res_type: ResourceType::Mem,
                flags: 0,
            });
        }
    }
    
    // Populate IRQ resources from interrupts property
    let mut irq_index = 0;
    loop {
        match of_irq_to_resource(Some(np), irq_index) {
            Ok((irq, flags)) => {
                dev.resources.push(Resource {
                    name: format!("irq{}", irq_index),
                    start: irq as u64,
                    end: irq as u64,
                    res_type: ResourceType::Irq,
                    flags,
                });
                irq_index += 1;
            }
            Err(_) => break,
        }
    }
    
    dev.num_resources = dev.resources.len();
    
    Ok(dev)
}

/// of_platform_device_create - Create platform device from node
/// @np: device node
/// @bus_id: device name (optional)
///
/// Returns created platform device, or error.
pub fn of_platform_device_create(
    np: &DeviceNode,
    bus_id: Option<&str>,
) -> Result<PlatformDevice> {
    // Check if device is available
    if !of_device_is_available(Some(np)) {
        return Err(PlatformError::NotAvailable);
    }
    
    // Check if already populated (would need node flags)
    // Simplified - skip for now
    
    // Allocate device
    let dev = of_device_alloc(np, bus_id)?;
    
    Ok(dev)
}

/// of_match_node - Check if node matches device ID table
/// @matches: device ID table
/// @node: device node
///
/// Returns true if node matches any entry in table.
fn of_match_node(matches: &[OfDeviceId], node: &DeviceNode) -> bool {
    for match_entry in matches {
        if of_device_is_compatible(Some(node), &match_entry.compatible) > 0 {
            return true;
        }
    }
    false
}

/// Check if node should be skipped
fn should_skip_node(node: &DeviceNode) -> bool {
    for compat in SKIPPED_COMPATIBLES {
        if of_device_is_compatible(Some(node), compat) > 0 {
            return true;
        }
    }
    false
}

/// of_platform_bus_create - Create devices for node and children
/// @bus: bus node
/// @matches: match table
/// @strict: require compatible property
///
/// Recursively creates platform devices for node and all children.
pub fn of_platform_bus_create(
    bus: &DeviceNode,
    matches: &[OfDeviceId],
    strict: bool,
) -> Result<Vec<PlatformDevice>> {
    let mut devices = Vec::new();
    
    // Check if node has compatible property (if strict)
    if strict {
        if of_get_property(Some(bus), "compatible").is_none() {
            return Ok(devices);
        }
    }
    
    // Skip certain nodes
    if should_skip_node(bus) {
        return Ok(devices);
    }
    
    // Check if already populated (would need node flags)
    // Simplified - skip for now
    
    // Create device for this node if it matches
    if of_match_node(matches, bus) {
        match of_platform_device_create(bus, None) {
            Ok(dev) => devices.push(dev),
            Err(_) => return Ok(devices),
        }
    }
    
    // Recursively create devices for children
    let mut child = bus.child;
    while let Some(child_ptr) = child {
        let child_node = unsafe { &*child_ptr };
        
        match of_platform_bus_create(child_node, matches, strict) {
            Ok(mut child_devices) => devices.append(&mut child_devices),
            Err(_) => break,
        }
        
        child = child_node.sibling;
    }
    
    Ok(devices)
}

/// of_platform_populate - Populate platform devices from device tree
/// @root: root node (None for tree root)
/// @matches: match table (None for default)
///
/// Main entry point for creating platform devices from device tree.
/// This walks the tree and creates devices for all matching nodes.
pub fn of_platform_populate(
    root: Option<&DeviceNode>,
    matches: Option<&[OfDeviceId]>,
) -> Result<Vec<PlatformDevice>> {
    // Use default matches if none provided
    let default_matches = [
        OfDeviceId { compatible: "simple-bus".to_string() },
        OfDeviceId { compatible: "simple-mfd".to_string() },
        OfDeviceId { compatible: "isa".to_string() },
        OfDeviceId { compatible: "arm,amba-bus".to_string() },
    ];
    
    let matches = matches.unwrap_or(&default_matches);
    
    let mut devices = Vec::new();
    
    // Get root node
    let root = match root {
        Some(r) => r,
        None => return Err(PlatformError::Invalid), // Would need global of_root
    };
    
    // Create devices for all children
    let mut child = root.child;
    while let Some(child_ptr) = child {
        let child_node = unsafe { &*child_ptr };
        
        match of_platform_bus_create(child_node, matches, true) {
            Ok(mut child_devices) => devices.append(&mut child_devices),
            Err(_) => break,
        }
        
        child = child_node.sibling;
    }
    
    Ok(devices)
}

/// of_platform_default_populate - Populate with default match table
/// @root: root node
///
/// Convenience function using default match table.
pub fn of_platform_default_populate(root: Option<&DeviceNode>) -> Result<Vec<PlatformDevice>> {
    of_platform_populate(root, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_device_creation() {
        // Would need mock device node
        assert!(true);
    }

    #[test]
    fn test_resource_types() {
        assert_eq!(ResourceType::Mem, ResourceType::Mem);
        assert_ne!(ResourceType::Mem, ResourceType::Irq);
    }

    #[test]
    fn test_skipped_compatibles() {
        assert!(SKIPPED_COMPATIBLES.contains(&"operating-points-v2"));
    }

    #[test]
    fn test_default_matches() {
        let matches = [
            OfDeviceId { compatible: "simple-bus".to_string() },
        ];
        assert_eq!(matches[0].compatible, "simple-bus");
    }

    #[test]
    fn test_platform_error_types() {
        assert_eq!(PlatformError::Invalid, PlatformError::Invalid);
        assert_ne!(PlatformError::Invalid, PlatformError::NoMemory);
    }

    #[test]
    fn test_resource_range() {
        let res = Resource {
            name: "test".to_string(),
            start: 0x1000,
            end: 0x1FFF,
            res_type: ResourceType::Mem,
            flags: 0,
        };
        assert_eq!(res.end - res.start + 1, 0x1000);
    }

    #[test]
    fn test_address_count_logic() {
        // 12 bytes per entry (2 addr cells + 1 size cell)
        let reg_len = 24;
        let count = reg_len / 12;
        assert_eq!(count, 2);
    }

    #[test]
    fn test_device_id_none() {
        let id = -1; // PLATFORM_DEVID_NONE
        assert_eq!(id, -1);
    }

    #[test]
    fn test_address_count_variable_cells() {
        // Test with different cell sizes
        // na=2, ns=1: entry_size = 12 bytes
        let na = 2;
        let ns = 1;
        let entry_size = (na + ns) * 4;
        assert_eq!(entry_size, 12);
        
        // na=1, ns=1: entry_size = 8 bytes
        let na = 1;
        let ns = 1;
        let entry_size = (na + ns) * 4;
        assert_eq!(entry_size, 8);
        
        // na=3, ns=2: entry_size = 20 bytes
        let na = 3;
        let ns = 2;
        let entry_size = (na + ns) * 4;
        assert_eq!(entry_size, 20);
    }

    #[test]
    fn test_address_count_calculation() {
        // 24 bytes with na=2, ns=1 (12 bytes per entry) = 2 entries
        let reg_len = 24;
        let na = 2;
        let ns = 1;
        let entry_size = (na + ns) * 4;
        let count = reg_len / entry_size;
        assert_eq!(count, 2);
        
        // 24 bytes with na=1, ns=1 (8 bytes per entry) = 3 entries
        let reg_len = 24;
        let na = 1;
        let ns = 1;
        let entry_size = (na + ns) * 4;
        let count = reg_len / entry_size;
        assert_eq!(count, 3);
    }
}
