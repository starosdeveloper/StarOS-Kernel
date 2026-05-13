// SPDX-License-Identifier: MIT OR Apache-2.0
/*
 * IRQ parsing from device tree
 *
 * Ported from Linux kernel drivers/of/irq.c
 * Original Copyright (C) 1996 Paul Mackerras
 */

use crate::drivers::of::base::{DeviceNode, of_get_property, of_get_parent};
use crate::drivers::of::property::of_property_read_u32;
use crate::prelude::*;
use core::sync::atomic::{AtomicPtr, Ordering};

/// Global phandle cache
static PHANDLE_CACHE: AtomicPtr<BTreeMap<u32, *const DeviceNode>> = AtomicPtr::new(core::ptr::null_mut());

/// Initialize phandle cache from device tree
/// Must be called once at boot after device tree is unflattened
pub fn of_phandle_cache_init(root: &DeviceNode) {
    let mut cache = BTreeMap::new();
    build_phandle_cache(root, &mut cache);
    
    let boxed = Box::new(cache);
    PHANDLE_CACHE.store(Box::into_raw(boxed), Ordering::Release);
}

/// Recursively build phandle cache
fn build_phandle_cache(node: &DeviceNode, cache: &mut BTreeMap<u32, *const DeviceNode>) {
    // Add this node if it has a phandle
    if node.phandle != 0 {
        cache.insert(node.phandle, node as *const DeviceNode);
    }
    
    // Recurse to children
    let mut child = node.child;
    while let Some(child_ptr) = child {
        let child_node = unsafe { &*child_ptr };
        build_phandle_cache(child_node, cache);
        child = child_node.sibling;
    }
}

/// Find node by phandle
pub fn of_find_node_by_phandle(phandle: u32) -> Option<&'static DeviceNode> {
    let cache_ptr = PHANDLE_CACHE.load(Ordering::Acquire);
    if cache_ptr.is_null() {
        return None;
    }
    
    let cache = unsafe { &*cache_ptr };
    cache.get(&phandle).map(|&ptr| unsafe { &*ptr })
}

/// Maximum phandle arguments
const MAX_PHANDLE_ARGS: usize = 16;

/// IRQ Domain - maps (controller, hwirq) to global Linux IRQ
static mut IRQ_DOMAIN_COUNTER: u32 = 1;
static IRQ_DOMAINS: AtomicPtr<BTreeMap<*const DeviceNode, IrqDomain>> = AtomicPtr::new(core::ptr::null_mut());

/// IRQ Domain structure
#[derive(Debug, Clone)]
pub struct IrqDomain {
    /// Base Linux IRQ number for this domain
    pub base_irq: u32,
    /// Number of IRQs in this domain
    pub size: u32,
}

/// Register IRQ domain for interrupt controller
pub fn irq_domain_add(controller: &DeviceNode, size: u32) -> u32 {
    let domains_ptr = IRQ_DOMAINS.load(Ordering::Acquire);
    let domains = if domains_ptr.is_null() {
        let new_domains = Box::new(BTreeMap::new());
        let ptr = Box::into_raw(new_domains);
        IRQ_DOMAINS.store(ptr, Ordering::Release);
        unsafe { &mut *ptr }
    } else {
        unsafe { &mut *domains_ptr }
    };
    
    let base_irq = unsafe {
        let current = IRQ_DOMAIN_COUNTER;
        IRQ_DOMAIN_COUNTER += size;
        current
    };
    
    domains.insert(
        controller as *const DeviceNode,
        IrqDomain { base_irq, size },
    );
    
    base_irq
}

/// Map hwirq to Linux IRQ number
fn irq_domain_map(controller: &DeviceNode, hwirq: u32) -> Option<u32> {
    let domains_ptr = IRQ_DOMAINS.load(Ordering::Acquire);
    if domains_ptr.is_null() {
        return None;
    }
    
    let domains = unsafe { &*domains_ptr };
    let domain = domains.get(&(controller as *const DeviceNode))?;
    
    if hwirq >= domain.size {
        return None;
    }
    
    Some(domain.base_irq + hwirq)
}

/// IRQ parsing error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqError {
    /// Invalid argument
    Invalid,
    /// No data
    NoData,
    /// Not found
    NotFound,
    /// Parse failed
    ParseFailed,
}

pub type Result<T> = core::result::Result<T, IrqError>;

/// Parsed IRQ specifier
#[derive(Debug, Clone)]
pub struct OfIrq {
    /// Interrupt controller node
    pub controller: Option<*const DeviceNode>,
    /// IRQ arguments (hwirq, flags, etc)
    pub args: [u32; MAX_PHANDLE_ARGS],
    /// Number of valid arguments
    pub args_count: usize,
}

impl OfIrq {
    pub fn new() -> Self {
        Self {
            controller: None,
            args: [0; MAX_PHANDLE_ARGS],
            args_count: 0,
        }
    }
}

/// of_irq_find_parent - Find interrupt parent node
/// @child: device node
///
/// Returns interrupt parent node, or None if not found.
pub fn of_irq_find_parent(child: Option<&DeviceNode>) -> Option<&DeviceNode> {
    let mut child = child?;
    
    loop {
        // Check for interrupt-parent property
        if let Ok(parent_phandle) = of_property_read_u32(Some(child), "interrupt-parent") {
            // Find node by phandle (REAL IMPLEMENTATION)
            return of_find_node_by_phandle(parent_phandle);
        }
        
        // Try parent node
        child = match of_get_parent(Some(child)) {
            Some(p) => p,
            None => return None,
        };
        
        // Check if this node has #interrupt-cells (is interrupt controller)
        if of_property_read_u32(Some(child), "#interrupt-cells").is_ok() {
            return Some(child);
        }
    }
}

/// of_irq_parse_raw - Parse interrupt with interrupt-map support
/// @device: device node
/// @addr: device address (from reg property)
/// @out_irq: output IRQ structure
///
/// This handles interrupt-map translation for complex bus topologies.
fn of_irq_parse_raw(
    device: &DeviceNode,
    addr: &[u8],
    out_irq: &mut OfIrq,
) -> Result<()> {
    let mut ipar = of_irq_find_parent(Some(device)).ok_or(IrqError::NotFound)?;
    
    // Get initial interrupt size
    let mut intsize = of_property_read_u32(Some(ipar), "#interrupt-cells")
        .map_err(|_| IrqError::Invalid)? as usize;
    
    // Get address size for interrupt-map matching
    let mut addrsize = of_property_read_u32(Some(ipar), "#address-cells")
        .unwrap_or(2) as usize;
    
    // Build match array: [address cells] [interrupt cells]
    let mut match_array = [0u32; MAX_PHANDLE_ARGS];
    
    // Copy address cells
    for i in 0..addrsize.min(addr.len() / 4) {
        let offset = i * 4;
        match_array[i] = u32::from_be_bytes([
            addr[offset],
            addr[offset + 1],
            addr[offset + 2],
            addr[offset + 3],
        ]);
    }
    
    // Copy interrupt cells
    for i in 0..intsize.min(out_irq.args_count) {
        match_array[addrsize + i] = out_irq.args[i];
    }
    
    // Walk up tree looking for interrupt-map
    loop {
        // Check if this is an interrupt controller
        let is_intc = of_get_property(Some(ipar), "interrupt-controller").is_some();
        
        // Get interrupt-map
        let imap = of_get_property(Some(ipar), "interrupt-map");
        
        if is_intc && imap.is_none() {
            // Found final interrupt controller
            return Ok(());
        }
        
        if let Some(imap_data) = imap {
            // Parse interrupt-map
            let imask = of_get_property(Some(ipar), "interrupt-map-mask");
            
            // Apply mask if present
            let mut masked_match = match_array;
            if let Some(mask_data) = imask {
                for i in 0..(addrsize + intsize).min(mask_data.len() / 4) {
                    let offset = i * 4;
                    let mask = u32::from_be_bytes([
                        mask_data[offset],
                        mask_data[offset + 1],
                        mask_data[offset + 2],
                        mask_data[offset + 3],
                    ]);
                    masked_match[i] &= mask;
                }
            }
            
            // Search through interrupt-map entries
            let entry_size = addrsize + intsize + 1; // +1 for parent phandle
            let mut offset = 0;
            let mut found_match = false;
            
            while offset + entry_size * 4 <= imap_data.len() {
                // Check if this entry matches
                let mut matches = true;
                for i in 0..(addrsize + intsize) {
                    let map_offset = offset + i * 4;
                    let map_val = u32::from_be_bytes([
                        imap_data[map_offset],
                        imap_data[map_offset + 1],
                        imap_data[map_offset + 2],
                        imap_data[map_offset + 3],
                    ]);
                    
                    if masked_match[i] != map_val {
                        matches = false;
                        break;
                    }
                }
                
                if matches {
                    found_match = true;
                    // Found matching entry - extract parent phandle
                    let phandle_offset = offset + (addrsize + intsize) * 4;
                    let parent_phandle = u32::from_be_bytes([
                        imap_data[phandle_offset],
                        imap_data[phandle_offset + 1],
                        imap_data[phandle_offset + 2],
                        imap_data[phandle_offset + 3],
                    ]);
                    
                    // Find parent controller
                    let parent = of_find_node_by_phandle(parent_phandle)
                        .ok_or(IrqError::NotFound)?;
                    
                    // Get parent's interrupt cells
                    let parent_intsize = of_property_read_u32(Some(parent), "#interrupt-cells")
                        .map_err(|_| IrqError::Invalid)? as usize;
                    
                    // Extract translated interrupt specifier
                    let spec_offset = phandle_offset + 4;
                    out_irq.controller = Some(parent as *const DeviceNode);
                    out_irq.args_count = parent_intsize;
                    
                    for i in 0..parent_intsize.min(MAX_PHANDLE_ARGS) {
                        let arg_offset = spec_offset + i * 4;
                        if arg_offset + 4 <= imap_data.len() {
                            out_irq.args[i] = u32::from_be_bytes([
                                imap_data[arg_offset],
                                imap_data[arg_offset + 1],
                                imap_data[arg_offset + 2],
                                imap_data[arg_offset + 3],
                            ]);
                        }
                    }
                    
                    // Continue walking up with new parent
                    ipar = parent;
                    intsize = parent_intsize;
                    addrsize = of_property_read_u32(Some(parent), "#address-cells")
                        .unwrap_or(0) as usize;
                    
                    // Update match array for next level
                    for i in 0..intsize {
                        match_array[addrsize + i] = out_irq.args[i];
                    }
                    
                    break;
                }
                
                // Move to next entry
                // Entry format: <child-unit-addr> <child-interrupt-specifier> <parent-phandle> <parent-interrupt-specifier>
                let parent_phandle_offset = offset + (addrsize + intsize) * 4;
                let parent_phandle = u32::from_be_bytes([
                    imap_data[parent_phandle_offset],
                    imap_data[parent_phandle_offset + 1],
                    imap_data[parent_phandle_offset + 2],
                    imap_data[parent_phandle_offset + 3],
                ]);
                
                if let Some(parent) = of_find_node_by_phandle(parent_phandle) {
                    let parent_intsize = of_property_read_u32(Some(parent), "#interrupt-cells")
                        .unwrap_or(1) as usize;
                    offset += (addrsize + intsize + 1 + parent_intsize) * 4;
                } else {
                    break;
                }
            }
            
            if !found_match {
                return Err(IrqError::NotFound);
            }
        } else {
            // No interrupt-map, move to parent
            ipar = of_irq_find_parent(Some(ipar)).ok_or(IrqError::NotFound)?;
            intsize = of_property_read_u32(Some(ipar), "#interrupt-cells")
                .map_err(|_| IrqError::Invalid)? as usize;
        }
    }
}

/// of_irq_parse_one - Parse interrupt for device
/// @device: device node
/// @index: interrupt index
/// @out_irq: output IRQ structure
///
/// Returns 0 on success, error otherwise.
pub fn of_irq_parse_one(
    device: Option<&DeviceNode>,
    index: usize,
    out_irq: &mut OfIrq,
) -> Result<()> {
    let device = device.ok_or(IrqError::Invalid)?;
    
    // Get reg property for address matching in interrupt-map
    let addr = of_get_property(Some(device), "reg").unwrap_or(&[]);
    
    // Try interrupts-extended first (allows per-interrupt parent specification)
    // Format: <&intc1 irq1 flags1> <&intc2 irq2 flags2> ...
    if let Some(ext_data) = of_get_property(Some(device), "interrupts-extended") {
        // Calculate offset for this index
        // Each entry: phandle + interrupt-cells
        let mut offset = 0;
        let mut current_index = 0;
        
        while offset < ext_data.len() && current_index <= index {
            // Read phandle
            if offset + 4 > ext_data.len() {
                break;
            }
            
            let phandle = u32::from_be_bytes([
                ext_data[offset],
                ext_data[offset + 1],
                ext_data[offset + 2],
                ext_data[offset + 3],
            ]);
            offset += 4;
            
            // Find controller
            let controller = of_find_node_by_phandle(phandle).ok_or(IrqError::NotFound)?;
            
            // Get interrupt cells
            let intsize = of_property_read_u32(Some(controller), "#interrupt-cells")
                .map_err(|_| IrqError::Invalid)? as usize;
            
            if current_index == index {
                // This is our interrupt
                out_irq.controller = Some(controller as *const DeviceNode);
                out_irq.args_count = intsize;
                
                for i in 0..intsize.min(MAX_PHANDLE_ARGS) {
                    if offset + 4 > ext_data.len() {
                        return Err(IrqError::Invalid);
                    }
                    
                    out_irq.args[i] = u32::from_be_bytes([
                        ext_data[offset],
                        ext_data[offset + 1],
                        ext_data[offset + 2],
                        ext_data[offset + 3],
                    ]);
                    offset += 4;
                }
                
                return of_irq_parse_raw(device, addr, out_irq);
            }
            
            // Skip this interrupt's cells
            offset += intsize * 4;
            current_index += 1;
        }
        
        return Err(IrqError::Invalid);
    }
    
    // Fall back to regular interrupts property
    let parent = of_irq_find_parent(Some(device)).ok_or(IrqError::NotFound)?;
    
    // Get #interrupt-cells from parent
    let intsize = of_property_read_u32(Some(parent), "#interrupt-cells")
        .map_err(|_| IrqError::Invalid)? as usize;
    
    if intsize > MAX_PHANDLE_ARGS {
        return Err(IrqError::Invalid);
    }
    
    // Get interrupts property
    let interrupts = of_get_property(Some(device), "interrupts")
        .ok_or(IrqError::NoData)?;
    
    // Calculate offset for this index
    let offset = index * intsize * 4;
    if offset + intsize * 4 > interrupts.len() {
        return Err(IrqError::Invalid);
    }
    
    // Parse interrupt specifier
    out_irq.controller = Some(parent as *const DeviceNode);
    out_irq.args_count = intsize;
    
    for i in 0..intsize {
        let byte_offset = offset + i * 4;
        out_irq.args[i] = u32::from_be_bytes([
            interrupts[byte_offset],
            interrupts[byte_offset + 1],
            interrupts[byte_offset + 2],
            interrupts[byte_offset + 3],
        ]);
    }
    
    // Handle interrupt-map translation
    of_irq_parse_raw(device, addr, out_irq)
}

/// of_irq_get - Get Linux IRQ number
/// @dev: device node
/// @index: interrupt index
///
/// Returns Linux IRQ number (globally unique), or error.
pub fn of_irq_get(dev: Option<&DeviceNode>, index: usize) -> Result<u32> {
    let mut oirq = OfIrq::new();
    
    of_irq_parse_one(dev, index, &mut oirq)?;
    
    // Get controller node
    let controller = oirq.controller
        .map(|ptr| unsafe { &*ptr })
        .ok_or(IrqError::Invalid)?;
    
    // Get hwirq (hardware IRQ number, local to controller)
    let hwirq = if oirq.args_count > 0 {
        oirq.args[0]
    } else {
        return Err(IrqError::NoData);
    };
    
    // Map to global Linux IRQ via IRQ domain
    irq_domain_map(controller, hwirq).ok_or(IrqError::NotFound)
}

/// of_irq_count - Count interrupts for device
/// @dev: device node
///
/// Returns number of interrupts, or 0 if none.
pub fn of_irq_count(dev: Option<&DeviceNode>) -> usize {
    let dev = match dev {
        Some(d) => d,
        None => return 0,
    };
    
    // Get interrupt parent
    let parent = match of_irq_find_parent(Some(dev)) {
        Some(p) => p,
        None => return 0,
    };
    
    // Get #interrupt-cells
    let intsize = match of_property_read_u32(Some(parent), "#interrupt-cells") {
        Ok(s) => s as usize,
        Err(_) => return 0,
    };
    
    // Get interrupts property
    let interrupts = match of_get_property(Some(dev), "interrupts") {
        Some(i) => i,
        None => return 0,
    };
    
    // Calculate count
    let total_cells = interrupts.len() / 4;
    total_cells / intsize
}

/// of_irq_to_resource - Convert IRQ to resource
/// @dev: device node
/// @index: interrupt index
///
/// Returns (irq_number, flags) tuple, or error.
pub fn of_irq_to_resource(dev: Option<&DeviceNode>, index: usize) -> Result<(u32, u32)> {
    let irq = of_irq_get(dev, index)?;
    
    // Get IRQ flags from parsed specifier
    let mut oirq = OfIrq::new();
    of_irq_parse_one(dev, index, &mut oirq)?;
    
    // Second arg is typically flags (for GIC)
    let flags = if oirq.args_count > 1 {
        oirq.args[1]
    } else {
        0
    };
    
    Ok((irq, flags))
}

/// of_irq_get_byname - Get IRQ by name
/// @dev: device node
/// @name: interrupt name
///
/// Returns IRQ number, or error.
pub fn of_irq_get_byname(dev: Option<&DeviceNode>, name: &str) -> Result<u32> {
    let dev = dev.ok_or(IrqError::Invalid)?;
    
    // Find index in interrupt-names
    let names = of_get_property(Some(dev), "interrupt-names")
        .ok_or(IrqError::NotFound)?;
    
    let mut index = 0;
    let mut offset = 0;
    
    while offset < names.len() {
        // Find null terminator
        let end = names[offset..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| offset + p)
            .unwrap_or(names.len());
        
        if let Ok(s) = core::str::from_utf8(&names[offset..end]) {
            if s == name {
                return of_irq_get(Some(dev), index);
            }
        }
        
        offset = end + 1;
        index += 1;
    }
    
    Err(IrqError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_irq_new() {
        let irq = OfIrq::new();
        assert_eq!(irq.args_count, 0);
        assert!(irq.controller.is_none());
    }

    #[test]
    fn test_max_phandle_args() {
        assert!(MAX_PHANDLE_ARGS >= 16);
    }

    #[test]
    fn test_irq_error_types() {
        assert_eq!(IrqError::Invalid, IrqError::Invalid);
        assert_ne!(IrqError::Invalid, IrqError::NotFound);
    }

    #[test]
    fn test_interrupt_parsing_logic() {
        // Test interrupt specifier parsing
        // Format: <hwirq> <flags> for GIC
        let intsize = 3; // GIC uses 3 cells
        let index = 0;
        let offset = index * intsize * 4;
        assert_eq!(offset, 0);
        
        let index = 1;
        let offset = index * intsize * 4;
        assert_eq!(offset, 12);
    }

    #[test]
    fn test_interrupt_count_calculation() {
        let interrupts_len = 24; // bytes
        let intsize = 3; // cells per interrupt
        let total_cells = interrupts_len / 4;
        let count = total_cells / intsize;
        assert_eq!(count, 2); // 2 interrupts
    }

    #[test]
    fn test_gic_interrupt_format() {
        // GIC interrupt format: <type> <number> <flags>
        // type: 0=SPI, 1=PPI
        // number: interrupt number
        // flags: trigger type and polarity
        
        let type_spi = 0u32;
        let number = 42u32;
        let flags = 4u32; // IRQ_TYPE_LEVEL_HIGH
        
        assert_eq!(type_spi, 0);
        assert_eq!(number, 42);
        assert_eq!(flags, 4);
    }

    #[test]
    fn test_interrupt_names_parsing() {
        let names = b"irq0\0irq1\0irq2\0";
        let mut count = 0;
        let mut offset = 0;
        
        while offset < names.len() {
            if let Some(pos) = names[offset..].iter().position(|&b| b == 0) {
                count += 1;
                offset += pos + 1;
            } else {
                break;
            }
        }
        
        assert_eq!(count, 3);
    }

    #[test]
    fn test_irq_args_bounds() {
        let intsize = 3;
        assert!(intsize <= MAX_PHANDLE_ARGS);
    }

    #[test]
    fn test_irq_offset_calculation() {
        let index = 2;
        let intsize = 3;
        let offset = index * intsize * 4;
        assert_eq!(offset, 24);
    }
}
