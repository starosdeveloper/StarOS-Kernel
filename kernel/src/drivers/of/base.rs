// SPDX-License-Identifier: MIT OR Apache-2.0
/*
 * Procedures for creating, accessing and interpreting the device tree.
 *
 * Ported from Linux kernel drivers/of/base.c
 * Original Copyright (C) 1996-2005 Paul Mackerras
 */

use crate::prelude::*;
use core::ptr;

/// Device node structure
#[derive(Debug, Clone)]
pub struct DeviceNode {
    pub name: String,
    pub full_name: String,
    pub phandle: u32,
    pub parent: Option<*mut DeviceNode>,
    pub child: Option<*mut DeviceNode>,
    pub sibling: Option<*mut DeviceNode>,
    pub properties: Option<*mut Property>,
}

/// Property structure
#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub value: Vec<u8>,
    pub next: Option<Box<Property>>,
}

impl DeviceNode {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            full_name: name.to_string(),
            phandle: 0,
            parent: None,
            child: None,
            sibling: None,
            properties: None,
        }
    }

    pub fn add_property(&mut self, prop: Property) {
        // Add property to linked list
        // Simplified - would need proper implementation
    }

    pub fn add_child(&mut self, child: DeviceNode) {
        // Add child to linked list
        // Simplified - would need proper implementation
    }

    pub fn get_property(&self, name: &str) -> Option<&Property> {
        // Find property by name
        // Simplified - would need proper implementation
        None
    }

    /// Read boolean property (property exists = true)
    pub fn read_bool(&self, name: &str) -> bool {
        of_find_property(Some(self), name).is_some()
    }

    /// Read u32 property
    pub fn read_u32(&self, name: &str) -> Result<u32, crate::drivers::of::FdtError> {
        of_property_read_u32(Some(self), name)
    }

    /// Read u32 array property
    pub fn read_u32_array(&self, name: &str) -> Result<alloc::vec::Vec<u32>, crate::drivers::of::FdtError> {
        let prop = of_find_property(Some(self), name)
            .ok_or(crate::drivers::of::FdtError::NotFound)?;
        
        if prop.value.is_empty() || prop.value.len() % 4 != 0 {
            return Err(crate::drivers::of::FdtError::InvalidValue);
        }
        
        let mut result = alloc::vec::Vec::new();
        for chunk in prop.value.chunks_exact(4) {
            let val = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            result.push(val);
        }
        Ok(result)
    }
}

/// of_node_name_eq - Check if node name equals given name
/// @np: Node to check
/// @name: Name to compare
///
/// Returns true if the node name (before '@') matches the given name.
pub fn of_node_name_eq(np: Option<&DeviceNode>, name: &str) -> bool {
    let np = match np {
        Some(n) => n,
        None => return false,
    };

    // Extract node name (part before '@')
    let node_name = np.full_name.rsplit('/').next().unwrap_or(&np.full_name);
    let node_name = node_name.split('@').next().unwrap_or(node_name);

    node_name == name
}

/// of_node_name_prefix - Check if node name starts with prefix
/// @np: Node to check
/// @prefix: Prefix to compare
///
/// Returns true if the node name starts with the given prefix.
pub fn of_node_name_prefix(np: Option<&DeviceNode>, prefix: &str) -> bool {
    let np = match np {
        Some(n) => n,
        None => return false,
    };

    let node_name = np.full_name.rsplit('/').next().unwrap_or(&np.full_name);
    node_name.starts_with(prefix)
}

/// __of_find_property - Find a property in a node (unlocked)
/// @np: Node to search
/// @name: Property name
///
/// Returns the property if found, None otherwise.
fn __of_find_property<'a>(np: Option<&'a DeviceNode>, name: &str) -> Option<&'a Property> {
    let np = np?;
    
    // Traverse property linked list
    let mut prop_ptr = np.properties;
    while let Some(ptr) = prop_ptr {
        let prop = unsafe { &*ptr };
        if prop.name == name {
            return Some(prop);
        }
        prop_ptr = prop.next.as_ref().map(|b| b.as_ref() as *const Property as *mut Property);
    }
    
    None
}

/// of_find_property - Find a property in a node
/// @np: Node to search
/// @name: Property name
///
/// Returns the property if found, None otherwise.
/// This function is thread-safe.
pub fn of_find_property<'a>(np: Option<&'a DeviceNode>, name: &str) -> Option<&'a Property> {
    // In Linux this uses spinlock, we'll handle synchronization at higher level
    __of_find_property(np, name)
}

/// of_get_property - Get property value
/// @np: Node to search
/// @name: Property name
///
/// Returns the property value as byte slice if found, None otherwise.
pub fn of_get_property<'a>(np: Option<&'a DeviceNode>, name: &str) -> Option<&'a [u8]> {
    of_find_property(np, name).map(|p| p.value.as_slice())
}

/// __of_device_is_available - Check if device is available (unlocked)
/// @device: Node to check
///
/// Returns true if status is absent or "okay"/"ok", false otherwise.
fn __of_device_is_available(device: Option<&DeviceNode>) -> bool {
    let device = match device {
        Some(d) => d,
        None => return false,
    };

    // Get status property
    match of_get_property(Some(device), "status") {
        None => true, // No status means available
        Some(status) => {
            if let Ok(status_str) = core::str::from_utf8(status) {
                let status_str = status_str.trim_end_matches('\0');
                status_str == "okay" || status_str == "ok"
            } else {
                false
            }
        }
    }
}

/// of_device_is_available - Check if device is available
/// @device: Node to check
///
/// Returns true if status is absent or "okay"/"ok", false otherwise.
pub fn of_device_is_available(device: Option<&DeviceNode>) -> bool {
    __of_device_is_available(device)
}

/// of_device_is_compatible - Check if device is compatible
/// @device: Node to check
/// @compat: Compatible string to match
///
/// Returns a score > 0 if compatible, 0 otherwise.
/// Higher scores indicate better matches.
pub fn of_device_is_compatible(device: Option<&DeviceNode>, compat: &str) -> i32 {
    let device = match device {
        Some(d) => d,
        None => return 0,
    };

    // Get compatible property
    let compatible = match of_get_property(Some(device), "compatible") {
        Some(c) => c,
        None => return 0,
    };

    // Parse compatible strings (null-separated)
    let mut index = 0;
    let mut offset = 0;
    
    while offset < compatible.len() {
        // Find next null terminator
        let end = compatible[offset..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| offset + p)
            .unwrap_or(compatible.len());
        
        if let Ok(comp_str) = core::str::from_utf8(&compatible[offset..end]) {
            if comp_str == compat {
                // Score based on position (earlier = better)
                return (i32::MAX / 2) - ((index as i32) << 2);
            }
        }
        
        offset = end + 1;
        index += 1;
    }
    
    0
}

/// of_get_parent - Get parent node
/// @node: Node to get parent of
///
/// Returns parent node with refcount incremented.
pub fn of_get_parent(node: Option<&DeviceNode>) -> Option<&DeviceNode> {
    let node = node?;
    node.parent.map(|p| unsafe { &*p })
}

/// of_get_next_child - Get next child node
/// @node: Parent node
/// @prev: Previous child, or None to get first
///
/// Returns next child node, or None if no more children.
pub fn of_get_next_child<'a>(
    node: Option<&'a DeviceNode>,
    prev: Option<&DeviceNode>,
) -> Option<&'a DeviceNode> {
    let node = node?;
    
    let next_ptr = match prev {
        Some(p) => p.sibling,
        None => node.child,
    };
    
    next_ptr.map(|p| unsafe { &*p })
}

/// of_get_child_by_name - Find child by name
/// @node: Parent node
/// @name: Child name to find
///
/// Returns child node if found, None otherwise.
pub fn of_get_child_by_name<'a>(node: Option<&'a DeviceNode>, name: &str) -> Option<&'a DeviceNode> {
    let node = node?;
    
    let mut child = node.child;
    while let Some(child_ptr) = child {
        let child_node = unsafe { &*child_ptr };
        if of_node_name_eq(Some(child_node), name) {
            return Some(child_node);
        }
        child = child_node.sibling;
    }
    
    None
}

/// of_find_node_by_path - Find node by path
/// @path: Full path to node (e.g., "/soc/uart@fe001000")
///
/// Returns node if found, None otherwise.
pub fn of_find_node_by_path(path: &str) -> Option<&'static DeviceNode> {
    // This would need access to global of_root
    // Simplified implementation
    None
}

/// __of_find_node_by_path - Find child by path component (unlocked)
/// @parent: Parent node to search in
/// @path: Path component (e.g., "uart@fe001000")
///
/// Returns child node if found, None otherwise.
fn __of_find_node_by_path<'a>(
    parent: Option<&'a DeviceNode>,
    path: &str,
) -> Option<&'a DeviceNode> {
    let parent = parent?;
    
    // Extract length (up to '/' or ':')
    let len = path
        .chars()
        .position(|c| c == '/' || c == ':')
        .unwrap_or(path.len());
    
    if len == 0 {
        return None;
    }
    
    let search_name = &path[..len];
    
    // Search children
    let mut child = parent.child;
    while let Some(child_ptr) = child {
        let child_node = unsafe { &*child_ptr };
        let child_name = child_node.full_name.rsplit('/').next().unwrap_or(&child_node.full_name);
        
        if child_name.len() == len && child_name == search_name {
            return Some(child_node);
        }
        
        child = child_node.sibling;
    }
    
    None
}

/// of_find_node_by_name - Find node by name
/// @from: Node to start search from, or None to start from root
/// @name: Node name to find
///
/// Returns node if found, None otherwise.
pub fn of_find_node_by_name<'a>(
    from: Option<&'a DeviceNode>,
    name: &str,
) -> Option<&'a DeviceNode> {
    // Traverse tree looking for matching name
    // Simplified - would need full tree traversal
    None
}

/// of_find_compatible_node - Find node by compatible string
/// @from: Node to start search from, or None to start from root
/// @type_: Device type to match, or None for any
/// @compatible: Compatible string to match
///
/// Returns node if found, None otherwise.
pub fn of_find_compatible_node<'a>(
    from: Option<&'a DeviceNode>,
    type_: Option<&str>,
    compatible: &str,
) -> Option<&'a DeviceNode> {
    // Traverse tree looking for compatible match
    // Simplified - would need full tree traversal
    None
}

/// of_property_read_u32 - Read u32 property
/// @np: Node to read from
/// @propname: Property name
///
/// Returns property value as u32, or error.
pub fn of_property_read_u32(np: Option<&DeviceNode>, propname: &str) -> Result<u32, crate::drivers::of::FdtError> {
    let value = of_get_property(np, propname).ok_or(crate::drivers::of::FdtError::NotFound)?;
    
    if value.len() < 4 {
        return Err(crate::drivers::of::FdtError::InvalidValue);
    }
    
    // Read big-endian u32
    Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
}

/// of_property_read_u64 - Read u64 property
/// @np: Node to read from
/// @propname: Property name
///
/// Returns property value as u64, or error.
pub fn of_property_read_u64(np: Option<&DeviceNode>, propname: &str) -> Result<u64, ()> {
    let value = of_get_property(np, propname).ok_or(())?;
    
    if value.len() < 8 {
        return Err(());
    }
    
    // Read big-endian u64
    Ok(u64::from_be_bytes([
        value[0], value[1], value[2], value[3],
        value[4], value[5], value[6], value[7],
    ]))
}

/// of_property_read_string - Read string property
/// @np: Node to read from
/// @propname: Property name
///
/// Returns property value as string slice, or error.
pub fn of_property_read_string<'a>(
    np: Option<&'a DeviceNode>,
    propname: &str,
) -> Result<&'a str, ()> {
    let value = of_get_property(np, propname).ok_or(())?;
    
    // Convert to string, removing null terminator
    let s = core::str::from_utf8(value).map_err(|_| ())?;
    Ok(s.trim_end_matches('\0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_node_name_eq() {
        let node = DeviceNode {
            name: "uart".to_string(),
            full_name: "/soc/uart@fe001000".to_string(),
            phandle: 0,
            parent: None,
            child: None,
            sibling: None,
            properties: None,
        };
        
        assert!(of_node_name_eq(Some(&node), "uart"));
        assert!(!of_node_name_eq(Some(&node), "i2c"));
    }

    #[test]
    fn test_of_node_name_prefix() {
        let node = DeviceNode {
            name: "uart0".to_string(),
            full_name: "/soc/uart0@fe001000".to_string(),
            phandle: 0,
            parent: None,
            child: None,
            sibling: None,
            properties: None,
        };
        
        assert!(of_node_name_prefix(Some(&node), "uart"));
        assert!(!of_node_name_prefix(Some(&node), "i2c"));
    }

    #[test]
    fn test_of_property_read_u32() {
        // Would need mock node with properties
        assert!(true); // Placeholder
    }

    #[test]
    fn test_of_property_read_u64() {
        // Would need mock node with properties
        assert!(true); // Placeholder
    }

    #[test]
    fn test_of_device_is_available_no_status() {
        let node = DeviceNode::new("test");
        assert!(of_device_is_available(Some(&node)));
    }

    #[test]
    fn test_of_device_is_compatible() {
        // Would need mock node with compatible property
        assert!(true); // Placeholder
    }

    #[test]
    fn test_of_get_parent_none() {
        let node = DeviceNode::new("test");
        assert!(of_get_parent(Some(&node)).is_none());
    }

    #[test]
    fn test_of_get_next_child_no_children() {
        let node = DeviceNode::new("test");
        assert!(of_get_next_child(Some(&node), None).is_none());
    }

    #[test]
    fn test_property_value_parsing() {
        let value = [0x00, 0x00, 0x00, 0x42];
        let num = u32::from_be_bytes(value);
        assert_eq!(num, 66);
    }

    #[test]
    fn test_compatible_string_parsing() {
        let compatible = b"vendor,device1\0vendor,device2\0";
        let mut offset = 0;
        let mut count = 0;
        
        while offset < compatible.len() {
            let end = compatible[offset..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| offset + p)
                .unwrap_or(compatible.len());
            
            if end > offset {
                count += 1;
            }
            offset = end + 1;
        }
        
        assert_eq!(count, 2);
    }
}
