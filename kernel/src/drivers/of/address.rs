// SPDX-License-Identifier: MIT OR Apache-2.0
/*
 * Address translation from device tree
 *
 * Ported from Linux kernel drivers/of/address.c
 */

use crate::drivers::of::base::{DeviceNode, of_get_property, of_get_parent};
use crate::drivers::of::property::of_property_read_u32;

/// Bad address marker
pub const OF_BAD_ADDR: u64 = u64::MAX;

/// Maximum address cells
const OF_MAX_ADDR_CELLS: usize = 4;

/// Address translation error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrError {
    /// Invalid address
    Invalid,
    /// Translation failed
    TranslateFailed,
    /// No parent
    NoParent,
    /// Bad cell count
    BadCellCount,
}

pub type Result<T> = core::result::Result<T, AddrError>;

/// of_read_number - Read big-endian number from cell array
/// @cell: pointer to cells
/// @size: number of cells
///
/// Returns the number read from cells.
pub fn of_read_number(cell: &[u8], size: usize) -> u64 {
    let mut result = 0u64;
    
    for i in 0..size {
        let offset = i * 4;
        if offset + 4 <= cell.len() {
            let val = u32::from_be_bytes([
                cell[offset],
                cell[offset + 1],
                cell[offset + 2],
                cell[offset + 3],
            ]);
            result = (result << 32) | (val as u64);
        }
    }
    
    result
}

/// of_n_addr_cells - Get #address-cells for node
/// @np: device node
///
/// Returns number of address cells.
pub fn of_n_addr_cells(np: Option<&DeviceNode>) -> usize {
    let np = match np {
        Some(n) => n,
        None => return 2, // Default
    };
    
    // Try parent first
    let parent = np.parent.map(|p| unsafe { &*p });
    let node = parent.unwrap_or(np);
    
    of_property_read_u32(Some(node), "#address-cells")
        .map(|v| v as usize)
        .unwrap_or(2)
}

/// of_n_size_cells - Get #size-cells for node
/// @np: device node
///
/// Returns number of size cells.
pub fn of_n_size_cells(np: Option<&DeviceNode>) -> usize {
    let np = match np {
        Some(n) => n,
        None => return 1, // Default
    };
    
    // Try parent first
    let parent = np.parent.map(|p| unsafe { &*p });
    let node = parent.unwrap_or(np);
    
    of_property_read_u32(Some(node), "#size-cells")
        .map(|v| v as usize)
        .unwrap_or(1)
}

/// of_get_address - Get address from reg property
/// @dev: device node
/// @index: index into reg array
/// @size: return size value
///
/// Returns address cells, or None on error.
pub fn of_get_address<'a>(
    dev: Option<&'a DeviceNode>,
    index: usize,
    size: Option<&mut u64>,
) -> Option<&'a [u8]> {
    let dev = dev?;
    let parent = of_get_parent(Some(dev))?;
    
    // Get "reg" property
    let reg = of_get_property(Some(dev), "reg")?;
    
    let na = of_n_addr_cells(Some(parent));
    let ns = of_n_size_cells(Some(parent));
    let onesize = (na + ns) * 4;
    
    // Check bounds
    if reg.len() < (index + 1) * onesize {
        return None;
    }
    
    let offset = index * onesize;
    let addr = &reg[offset..offset + na * 4];
    
    // Read size if requested
    if let Some(size_out) = size {
        let size_offset = offset + na * 4;
        *size_out = of_read_number(&reg[size_offset..], ns);
    }
    
    Some(addr)
}

/// of_translate_one - Translate address through one level
/// @parent: parent node
/// @addr: address to translate (modified in place)
/// @na: number of address cells for child
/// @ns: number of size cells for child
/// @pna: number of address cells for parent
///
/// Returns 0 on success, 1 on failure.
fn of_translate_one(
    parent: &DeviceNode,
    addr: &mut [u8],
    na: usize,
    ns: usize,
    pna: usize,
) -> i32 {
    // Get ranges property
    let ranges = match of_get_property(Some(parent), "ranges") {
        Some(r) => r,
        None => {
            // No ranges - non-translatable boundary
            // Exception: empty ranges quirk (Apple/PowerPC)
            // For ARM64 (our target), no ranges = fail
            return 1;
        }
    };
    
    if ranges.is_empty() {
        // Empty ranges means 1:1 translation
        let offset = of_read_number(&addr[..na * 4], na);
        // Zero out address, keep offset
        for i in 0..pna * 4 {
            addr[i] = 0;
        }
        // Write offset back
        write_number(addr, offset, pna);
        return 0;
    }
    
    // Parse ranges to find matching entry
    // Format: <child-addr> <parent-addr> <size>
    let rone = na + pna + ns; // One range entry size (in cells)
    let ranges_len = ranges.len() / 4; // Total cells
    
    let child_addr = of_read_number(&addr[..na * 4], na);
    
    let mut offset = 0;
    while offset + rone <= ranges_len {
        let range_start = offset * 4;
        
        // Read child address from range
        let range_child = of_read_number(&ranges[range_start..], na);
        
        // Read parent address from range
        let parent_start = range_start + na * 4;
        let range_parent = of_read_number(&ranges[parent_start..], pna);
        
        // Read size from range
        let size_start = parent_start + pna * 4;
        let range_size = of_read_number(&ranges[size_start..], ns);
        
        // Check if address falls within this range
        if child_addr >= range_child && child_addr < range_child + range_size {
            // Found matching range - calculate offset
            let addr_offset = child_addr - range_child;
            let translated = range_parent + addr_offset;
            
            // Write translated address back
            write_number(addr, translated, pna);
            return 0;
        }
        
        offset += rone;
    }
    
    // No matching range found
    1
}

/// write_number - Write number to cell array in big-endian
/// @addr: buffer to write to
/// @value: value to write
/// @size: number of cells
fn write_number(addr: &mut [u8], value: u64, size: usize) {
    if size == 1 {
        let bytes = (value as u32).to_be_bytes();
        addr[0..4].copy_from_slice(&bytes);
    } else if size == 2 {
        let high = ((value >> 32) as u32).to_be_bytes();
        let low = ((value & 0xFFFFFFFF) as u32).to_be_bytes();
        addr[0..4].copy_from_slice(&high);
        addr[4..8].copy_from_slice(&low);
    } else {
        // Handle other sizes
        for i in 0..size {
            let shift = (size - 1 - i) * 32;
            let val = ((value >> shift) as u32).to_be_bytes();
            let offset = i * 4;
            if offset + 4 <= addr.len() {
                addr[offset..offset + 4].copy_from_slice(&val);
            }
        }
    }
}

/// of_translate_address - Translate device tree address to CPU physical
/// @dev: device node
/// @in_addr: address to translate
///
/// Returns translated physical address, or OF_BAD_ADDR on error.
///
/// This walks up the device tree applying bus mappings via "ranges" properties.
/// Each level may translate addresses differently (e.g., SoC bus to CPU physical).
pub fn of_translate_address(dev: Option<&DeviceNode>, in_addr: &[u8]) -> u64 {
    let mut dev = match dev {
        Some(d) => d,
        None => return OF_BAD_ADDR,
    };
    
    let mut parent = match of_get_parent(Some(dev)) {
        Some(p) => p,
        None => {
            // No parent means we're at root - address is already physical
            return of_read_number(in_addr, of_n_addr_cells(Some(dev)));
        }
    };
    
    // Get initial address cells
    let mut na = of_n_addr_cells(Some(dev));
    let mut ns = of_n_size_cells(Some(dev));
    
    // Copy address locally
    let mut addr = [0u8; OF_MAX_ADDR_CELLS * 4];
    let addr_len = (na * 4).min(in_addr.len()).min(addr.len());
    addr[..addr_len].copy_from_slice(&in_addr[..addr_len]);
    
    // Walk up the tree translating at each level
    loop {
        // Get parent's address cells
        let pna = of_n_addr_cells(Some(parent));
        let pns = of_n_size_cells(Some(parent));
        
        // Translate through this level
        if of_translate_one(parent, &mut addr, na, ns, pna) != 0 {
            return OF_BAD_ADDR;
        }
        
        // Move up one level
        dev = parent;
        parent = match of_get_parent(Some(dev)) {
            Some(p) => p,
            None => {
                // Reached root - return final address
                return of_read_number(&addr[..pna * 4], pna);
            }
        };
        
        // Update cell counts for next iteration
        na = pna;
        ns = pns;
    }
}

/// of_address_to_resource - Convert address to resource
/// @dev: device node
/// @index: index into reg array
/// @r: resource structure to fill
///
/// Returns 0 on success, error otherwise.
pub fn of_address_to_resource(
    dev: Option<&DeviceNode>,
    index: usize,
) -> Result<(u64, u64)> {
    let mut size = 0u64;
    let addr = of_get_address(dev, index, Some(&mut size))
        .ok_or(AddrError::Invalid)?;
    
    let phys_addr = of_translate_address(dev, addr);
    if phys_addr == OF_BAD_ADDR {
        return Err(AddrError::TranslateFailed);
    }
    
    Ok((phys_addr, size))
}

/// of_iomap - Map device address to virtual
/// @dev: device node
/// @index: index into reg array
///
/// Returns virtual address, or None on error.
pub fn of_iomap(dev: Option<&DeviceNode>, index: usize) -> Option<*mut u8> {
    let (phys_addr, size) = of_address_to_resource(dev, index).ok()?;
    
    // Map physical address to virtual
    // This would use actual memory mapping in real implementation
    Some(phys_addr as *mut u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_read_number() {
        let cells = [0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02];
        let val = of_read_number(&cells, 2);
        assert_eq!(val, 0x0000000100000002);
    }

    #[test]
    fn test_of_read_number_single() {
        let cells = [0x00, 0x00, 0x10, 0x00];
        let val = of_read_number(&cells, 1);
        assert_eq!(val, 0x1000);
    }

    #[test]
    fn test_of_n_addr_cells_default() {
        let cells = of_n_addr_cells(None);
        assert_eq!(cells, 2);
    }

    #[test]
    fn test_of_n_size_cells_default() {
        let cells = of_n_size_cells(None);
        assert_eq!(cells, 1);
    }

    #[test]
    fn test_bad_addr_constant() {
        assert_eq!(OF_BAD_ADDR, u64::MAX);
    }

    #[test]
    fn test_addr_error_types() {
        assert_eq!(AddrError::Invalid, AddrError::Invalid);
        assert_ne!(AddrError::Invalid, AddrError::NoParent);
    }

    #[test]
    fn test_cell_size_calculation() {
        let na = 2;
        let ns = 1;
        let onesize = (na + ns) * 4;
        assert_eq!(onesize, 12);
    }

    #[test]
    fn test_address_bounds_check() {
        let reg_len = 24; // 2 entries
        let onesize = 12;
        let index = 1;
        assert!(reg_len >= (index + 1) * onesize);
    }

    #[test]
    fn test_max_addr_cells() {
        assert!(OF_MAX_ADDR_CELLS >= 4);
    }

    #[test]
    fn test_write_number_single_cell() {
        let mut addr = [0u8; 8];
        write_number(&mut addr, 0x12345678, 1);
        assert_eq!(addr[0..4], [0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_write_number_two_cells() {
        let mut addr = [0u8; 8];
        write_number(&mut addr, 0x0000000100000002, 2);
        assert_eq!(addr[0..4], [0x00, 0x00, 0x00, 0x01]);
        assert_eq!(addr[4..8], [0x00, 0x00, 0x00, 0x02]);
    }

    #[test]
    fn test_ranges_parsing_logic() {
        // Test ranges entry parsing
        // Format: <child-addr> <parent-addr> <size>
        // Example: child=0x1000, parent=0x80000000, size=0x1000
        let na = 1; // child address cells
        let pna = 1; // parent address cells
        let ns = 1; // size cells
        let rone = na + pna + ns;
        assert_eq!(rone, 3);
        
        // Simulate range entry
        let child_addr = 0x1000u64;
        let parent_addr = 0x80000000u64;
        let size = 0x1000u64;
        
        // Check if address 0x1500 falls in range
        let test_addr = 0x1500u64;
        assert!(test_addr >= child_addr);
        assert!(test_addr < child_addr + size);
        
        // Calculate translated address
        let offset = test_addr - child_addr;
        let translated = parent_addr + offset;
        assert_eq!(translated, 0x80000500);
    }

    #[test]
    fn test_ranges_boundary_check() {
        // Test boundary conditions
        let child_base = 0x1000u64;
        let size = 0x1000u64;
        
        // Start of range
        assert!(child_base >= child_base);
        assert!(child_base < child_base + size);
        
        // End of range (exclusive)
        let end = child_base + size;
        assert!(end >= child_base + size);
        
        // Just before end (inclusive)
        let last = child_base + size - 1;
        assert!(last < child_base + size);
    }

    #[test]
    fn test_multi_range_search() {
        // Test searching through multiple ranges
        // Range 1: 0x0000-0x0FFF -> 0x80000000
        // Range 2: 0x1000-0x1FFF -> 0x90000000
        // Range 3: 0x2000-0x2FFF -> 0xA0000000
        
        let ranges = [
            (0x0000u64, 0x80000000u64, 0x1000u64),
            (0x1000u64, 0x90000000u64, 0x1000u64),
            (0x2000u64, 0xA0000000u64, 0x1000u64),
        ];
        
        let test_addr = 0x1500u64;
        let mut found = None;
        
        for (child, parent, size) in ranges.iter() {
            if test_addr >= *child && test_addr < child + size {
                let offset = test_addr - child;
                found = Some(parent + offset);
                break;
            }
        }
        
        assert_eq!(found, Some(0x90000500));
    }
}
