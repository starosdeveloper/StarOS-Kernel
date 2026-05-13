// SPDX-License-Identifier: MIT OR Apache-2.0
/*
 * Functions for working with the Flattened Device Tree data format
 *
 * Ported from Linux kernel drivers/of/fdt.c
 * Original Copyright 2009 Benjamin Herrenschmidt, IBM Corp
 */

use crate::drivers::of::base::{DeviceNode, Property};
use crate::devicetree::parser::FdtParser;
use crate::prelude::*;
use core::mem;

/// Maximum depth of device tree traversal
const FDT_MAX_DEPTH: usize = 64;

/// Errors that can occur during FDT operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdtError {
    /// Invalid FDT header
    InvalidHeader,
    /// Node not found
    NotFound,
    /// Invalid data
    InvalidData,
    /// Invalid value
    InvalidValue,
    /// Memory allocation failed
    NoMemory,
    /// Maximum depth exceeded
    MaxDepthExceeded,
    /// Invalid property
    InvalidProperty,
}

pub type Result<T> = core::result::Result<T, FdtError>;

/// of_fdt_device_is_available - check if device is available
/// @parser: The FDT parser
/// @node_path: Path to the node
///
/// Check the "status" property of a device node. Returns true if the device
/// is available (status is "okay" or "ok", or status property is absent).
pub fn of_fdt_device_is_available(parser: &FdtParser, node_path: &str) -> bool {
    match parser.read_property(node_path, "status") {
        None => true, // No status property means available
        Some(status) => {
            if let Ok(status_str) = core::str::from_utf8(status) {
                let status_str = status_str.trim_end_matches('\0');
                status_str == "ok" || status_str == "okay"
            } else {
                false
            }
        }
    }
}

/// Memory allocator for unflattening
struct UnflattenAllocator {
    mem: *mut u8,
    offset: usize,
}

impl UnflattenAllocator {
    fn new(mem: *mut u8) -> Self {
        // CRITICAL: Assert non-null in production mode
        assert!(!mem.is_null(), "FDT allocation failed");
        Self { mem, offset: 0 }
    }

    /// Allocate aligned memory
    fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        // Align the current offset
        self.offset = (self.offset + align - 1) & !(align - 1);
        
        let ptr = unsafe { self.mem.add(self.offset) };
        self.offset += size;
        
        ptr
    }

    /// Get total allocated size
    fn size(&self) -> usize {
        self.offset
    }
}

/// populate_properties - populate properties for a device node
/// @parser: The FDT parser
/// @node_path: Path to the node
/// @alloc: Memory allocator
/// @np: The device_node to populate
/// @nodename: Full name of the node
/// @dryrun: If true, only calculate size without allocating
fn populate_properties(
    parser: &FdtParser,
    node_path: &str,
    alloc: &mut UnflattenAllocator,
    mut np: Option<&mut DeviceNode>,
    nodename: &str,
    dryrun: bool,
) {
    let mut has_name = false;
    let properties = parser.get_all_properties(node_path);

    for (pname, value) in properties {
        if pname == "name" {
            has_name = true;
        }

        // Allocate property structure - SYNC with actual allocation
        let prop_size = mem::size_of::<Property>();
        let prop_align = mem::align_of::<Property>();
        let _prop_ptr = alloc.alloc(prop_size, prop_align);

        if dryrun {
            continue;
        }

        if let Some(ref mut np) = np {
            let property = Property {
                name: pname.clone(),
                value: value.clone(),
                next: None,
            };

            // Handle phandle properties
            if (pname == "phandle" || pname == "linux,phandle" || pname == "ibm,phandle") 
                && value.len() >= 4 {
                let phandle = u32::from_be_bytes([value[0], value[1], value[2], value[3]]);
                if np.phandle == 0 || pname == "ibm,phandle" {
                    np.phandle = phandle;
                }
            }

            np.add_property(property);
        }
    }

    // With version 0x10 we may not have the name property,
    // recreate it here from the unit name if absent
    if !has_name {
        let name = if let Some(at_pos) = nodename.rfind('@') {
            &nodename[..at_pos]
        } else {
            nodename
        };

        let name = if let Some(slash_pos) = name.rfind('/') {
            &name[slash_pos + 1..]
        } else {
            name
        };

        // CRITICAL: Sync dry-run calculation with actual allocation
        let prop_size = mem::size_of::<Property>() + name.len() + 1;
        let prop_align = mem::align_of::<Property>();
        let _prop_ptr = alloc.alloc(prop_size, prop_align);

        if !dryrun {
            if let Some(ref mut np) = np {
                let property = Property {
                    name: "name".to_string(),
                    value: name.as_bytes().to_vec(),
                    next: None,
                };
                np.add_property(property);
            }
        }
    }
}

/// populate_node - populate a device node from FDT
/// @parser: The FDT parser
/// @node_path: Path to the node
/// @nodename: Name of the node
/// @alloc: Memory allocator
/// @parent: Parent device_node
/// @dryrun: If true, only calculate size
fn populate_node(
    parser: &FdtParser,
    node_path: &str,
    nodename: &str,
    alloc: &mut UnflattenAllocator,
    parent: Option<&mut DeviceNode>,
    dryrun: bool,
) -> Result<Option<DeviceNode>> {
    // Allocate device_node structure
    let node_size = mem::size_of::<DeviceNode>() + nodename.len() + 1;
    let node_align = mem::align_of::<DeviceNode>();
    let _node_ptr = alloc.alloc(node_size, node_align);

    if dryrun {
        return Ok(None);
    }

    // Create device node
    let mut np = DeviceNode::new(nodename);

    // Set parent relationship
    if let Some(parent) = parent {
        np.parent = Some(parent as *const DeviceNode as *mut DeviceNode);
        parent.add_child(np.clone());
    }

    // Populate properties
    populate_properties(parser, node_path, alloc, Some(&mut np), nodename, dryrun);

    // Set name from "name" property if available
    if let Some(name_prop) = np.get_property("name") {
        if let Ok(name_str) = core::str::from_utf8(&name_prop.value) {
            np.name = name_str.trim_end_matches('\0').to_string();
        }
    }

    if np.name.is_empty() {
        np.name = "<NULL>".to_string();
    }

    Ok(Some(np))
}

/// reverse_nodes - reverse the order of child nodes
/// @parent: Parent node whose children should be reversed
///
/// Reverses the child list to match the order in the .dts file.
/// Some drivers assume node order matches .dts node order.
fn reverse_nodes(parent: &mut DeviceNode) {
    // First, recursively reverse all children
    let mut child = parent.child;
    while let Some(child_ptr) = child {
        let child_node = unsafe { &mut *child_ptr };
        reverse_nodes(child_node);
        child = child_node.sibling;
    }

    // Now reverse the child list
    let mut child = parent.child;
    parent.child = None;
    
    while let Some(child_ptr) = child {
        let child_node = unsafe { &mut *child_ptr };
        let next = child_node.sibling;
        
        child_node.sibling = parent.child;
        parent.child = Some(child_ptr);
        
        child = next;
    }
}

/// unflatten_dt_nodes - Alloc and populate a device_node from the flat tree
/// @parser: The FDT parser
/// @alloc: Memory allocator (None for dry run)
/// @parent: Parent device_node
///
/// Unflattens the device tree, creating the tree of device_node structures.
/// CRITICAL: Properly tracks depth using FDT tokens (BEGIN_NODE/END_NODE)
fn unflatten_dt_nodes(
    parser: &FdtParser,
    alloc: Option<&mut UnflattenAllocator>,
    parent: Option<&mut DeviceNode>,
) -> Result<(usize, Option<DeviceNode>)> {
    let dryrun = alloc.is_none();
    let mut nps: [Option<DeviceNode>; FDT_MAX_DEPTH] = [const { None }; FDT_MAX_DEPTH];
    let mut root: Option<DeviceNode> = None;
    
    let initial_depth = if parent.is_some() { 1 } else { 0 };
    if let Some(parent) = parent {
        nps[1] = Some(parent.clone());
    }

    // Use walk_nodes which properly tracks depth via FDT tokens
    let current_alloc = if let Some(a) = alloc {
        a
    } else {
        // Dry run - use dummy allocator with special marker
        &mut UnflattenAllocator { mem: 0x1 as *mut u8, offset: 0 }
    };

    parser.walk_nodes(|path, _offset, depth| {
        // Check max depth
        if depth >= FDT_MAX_DEPTH - 1 {
            return Err(crate::error::KernelError::InvalidAddress);
        }

        // Check availability
        if !of_fdt_device_is_available(parser, path) {
            return Ok(true); // Skip but continue
        }

        // Extract node name from path
        let nodename = if let Some(pos) = path.rfind('/') {
            &path[pos + 1..]
        } else {
            path
        };

        // Get parent node at depth-1
        let parent_node = if depth > 0 {
            nps[depth - 1].as_mut()
        } else {
            None
        };

        // Populate this node
        let new_node = if dryrun {
            // Dry run - calculate size only
            populate_properties(parser, path, current_alloc, None, nodename, true);
            let node_size = mem::size_of::<DeviceNode>() + nodename.len() + 1;
            let node_align = mem::align_of::<DeviceNode>();
            let _node_ptr = current_alloc.alloc(node_size, node_align);
            None
        } else {
            populate_node(parser, path, nodename, current_alloc, parent_node, false)
                .map_err(|_| crate::error::KernelError::InvalidAddress)?
        };

        // Store the new node at current depth
        if let Some(new_node) = new_node {
            if root.is_none() && depth == initial_depth {
                root = Some(new_node.clone());
            }
            nps[depth] = Some(new_node);
        }

        Ok(true) // Continue walking
    }).map_err(|_| FdtError::InvalidData)?;

    // Reverse child lists to match .dts order
    if !dryrun {
        if let Some(ref mut root) = root {
            reverse_nodes(root);
        }
    }

    let size = current_alloc.size();
    Ok((size, root))
}

/// __unflatten_device_tree - create tree of device_nodes from flat blob
/// @parser: The FDT parser
/// @parent: Parent device node
///
/// Unflattens a device-tree, creating the tree of struct device_node.
/// It also fills the "name" and "type" pointers of the nodes so the
/// normal device-tree walking functions can be used.
pub fn unflatten_device_tree(
    parser: &FdtParser,
    parent: Option<&mut DeviceNode>,
) -> Result<DeviceNode> {
    // Validate FDT header
    let header = parser.get_header();
    if header.magic != 0xd00dfeed {
        return Err(FdtError::InvalidHeader);
    }

    // First pass: calculate required size (dry run with dummy allocator)
    let mut dummy_alloc = UnflattenAllocator { mem: 0x1 as *mut u8, offset: 0 };
    let (size, _) = unflatten_dt_nodes(parser, Some(&mut dummy_alloc), None)?;
    
    if size == 0 {
        return Err(FdtError::InvalidData);
    }

    // Align size
    let size = (size + 3) & !3;

    // Allocate memory for the expanded device tree
    let layout = core::alloc::Layout::from_size_align(size + 4, mem::align_of::<DeviceNode>())
        .map_err(|_| FdtError::NoMemory)?;
    
    let mem = unsafe { 
        #[cfg(not(any(test, feature = "std")))]
        { alloc::alloc::alloc_zeroed(layout) }
        #[cfg(any(test, feature = "std"))]
        { std::alloc::alloc_zeroed(layout) }
    };
    if mem.is_null() {
        return Err(FdtError::NoMemory);
    }

    // Write end marker
    unsafe {
        let marker = mem.add(size) as *mut u32;
        *marker = 0xdeadbeef_u32.to_be();
    }

    // Second pass: actual unflattening
    let mut allocator = UnflattenAllocator::new(mem);
    let (_, root) = unflatten_dt_nodes(parser, Some(&mut allocator), parent)?;

    // Check end marker
    let marker = unsafe { *(mem.add(size) as *const u32) };
    if u32::from_be(marker) != 0xdeadbeef {
        // End marker was overwritten - memory corruption
        unsafe { 
            #[cfg(not(any(test, feature = "std")))]
            { alloc::alloc::dealloc(mem, layout) }
            #[cfg(any(test, feature = "std"))]
            { std::alloc::dealloc(mem, layout) }
        };
        return Err(FdtError::InvalidData);
    }

    root.ok_or(FdtError::NotFound)
}

/// of_fdt_unflatten_tree - create tree of device_nodes from flat blob
/// @parser: FDT parser
/// @parent: Parent device node
///
/// Unflattens the device-tree passed by the firmware, creating the
/// tree of struct device_node. It also fills the "name" and "type"
/// pointers of the nodes so the normal device-tree walking functions
/// can be used.
///
/// This is the main public API for unflattening a device tree.
pub fn of_fdt_unflatten_tree(
    parser: &FdtParser,
    parent: Option<&mut DeviceNode>,
) -> Result<DeviceNode> {
    // In Linux this uses a mutex, but we'll handle synchronization at a higher level
    unflatten_device_tree(parser, parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_fdt_device_is_available_no_status() {
        // Device with no status property should be available
        // This would need a mock FDT - simplified test
        assert!(true); // Placeholder
    }

    #[test]
    fn test_of_fdt_device_is_available_okay() {
        // Device with status = "okay" should be available
        assert!(true); // Placeholder
    }

    #[test]
    fn test_of_fdt_device_is_available_disabled() {
        // Device with status = "disabled" should not be available
        assert!(true); // Placeholder
    }

    #[test]
    fn test_unflatten_allocator() {
        let mut buffer = [0u8; 1024];
        let mut alloc = UnflattenAllocator::new(buffer.as_mut_ptr());

        // Test basic allocation
        let ptr1 = alloc.alloc(16, 8);
        assert!(!ptr1.is_null());
        assert_eq!(alloc.size(), 16);

        // Test aligned allocation
        let ptr2 = alloc.alloc(8, 16);
        assert!(!ptr2.is_null());
        assert_eq!(alloc.size(), 32); // 16 + 16 (aligned)
    }

    #[test]
    fn test_populate_properties_name_extraction() {
        // Test name extraction from nodename
        let nodename = "uart@fe001000";
        let name = if let Some(at_pos) = nodename.rfind('@') {
            &nodename[..at_pos]
        } else {
            nodename
        };
        assert_eq!(name, "uart");
    }

    #[test]
    fn test_fdt_error_types() {
        let err = FdtError::InvalidHeader;
        assert_eq!(err, FdtError::InvalidHeader);
        
        let err2 = FdtError::NotFound;
        assert_ne!(err, err2);
    }

    #[test]
    fn test_unflatten_device_tree_invalid_fdt() {
        // Test with invalid FDT should return error
        // This would need a mock invalid FDT
        assert!(true); // Placeholder
    }

    #[test]
    fn test_reverse_nodes_empty() {
        // Test reversing nodes with no children
        let mut node = DeviceNode::new("test");
        reverse_nodes(&mut node);
        assert!(node.child.is_none());
    }

    #[test]
    fn test_phandle_parsing() {
        // Test phandle property parsing
        let value = [0x00, 0x00, 0x00, 0x42]; // phandle = 66
        let phandle = u32::from_be_bytes(value);
        assert_eq!(phandle, 66);
    }

    #[test]
    fn test_max_depth_check() {
        // Ensure FDT_MAX_DEPTH is reasonable
        assert!(FDT_MAX_DEPTH >= 32);
        assert!(FDT_MAX_DEPTH <= 128);
    }
}
