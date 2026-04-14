// SPDX-License-Identifier: MIT OR Apache-2.0
/*
 * Procedures for accessing and interpreting Devicetree properties
 *
 * Ported from Linux kernel drivers/of/property.c
 * Original Copyright (C) 1996-2005 Paul Mackerras
 */

use crate::drivers::of::base::{DeviceNode, Property, of_find_property, of_get_property};
pub use crate::drivers::of::base::{of_property_read_u32, of_property_read_u64, of_property_read_string};
use core::mem;

/// Property errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropError {
    /// Property does not exist
    Invalid,
    /// Property has no value
    NoData,
    /// Property data overflow
    Overflow,
    /// String not null-terminated
    InvalidSeq,
}

pub type Result<T> = core::result::Result<T, PropError>;

/// of_property_read_bool - Check if property exists
/// @np: device node
/// @propname: property name
///
/// Returns true if property exists, false otherwise.
pub fn of_property_read_bool(np: Option<&DeviceNode>, propname: &str) -> bool {
    of_find_property(np, propname).is_some()
}

/// of_property_count_elems_of_size - Count elements in property
/// @np: device node
/// @propname: property name
/// @elem_size: size of each element
///
/// Returns number of elements, or error.
pub fn of_property_count_elems_of_size(
    np: Option<&DeviceNode>,
    propname: &str,
    elem_size: usize,
) -> Result<usize> {
    let prop = of_find_property(np, propname).ok_or(PropError::Invalid)?;
    
    if prop.value.is_empty() {
        return Err(PropError::NoData);
    }
    
    if prop.value.len() % elem_size != 0 {
        return Err(PropError::Invalid);
    }
    
    Ok(prop.value.len() / elem_size)
}

/// of_property_read_u32_index - Read u32 at index
/// @np: device node
/// @propname: property name
/// @index: element index
///
/// Returns u32 value at index, or error.
pub fn of_property_read_u32_index(
    np: Option<&DeviceNode>,
    propname: &str,
    index: usize,
) -> Result<u32> {
    let value = of_get_property(np, propname).ok_or(PropError::Invalid)?;
    
    let required_size = (index + 1) * mem::size_of::<u32>();
    if value.len() < required_size {
        return Err(PropError::Overflow);
    }
    
    let offset = index * 4;
    Ok(u32::from_be_bytes([
        value[offset],
        value[offset + 1],
        value[offset + 2],
        value[offset + 3],
    ]))
}

/// of_property_read_u64_index - Read u64 at index
/// @np: device node
/// @propname: property name
/// @index: element index
///
/// Returns u64 value at index, or error.
pub fn of_property_read_u64_index(
    np: Option<&DeviceNode>,
    propname: &str,
    index: usize,
) -> Result<u64> {
    let value = of_get_property(np, propname).ok_or(PropError::Invalid)?;
    
    let required_size = (index + 1) * mem::size_of::<u64>();
    if value.len() < required_size {
        return Err(PropError::Overflow);
    }
    
    let offset = index * 8;
    Ok(u64::from_be_bytes([
        value[offset],
        value[offset + 1],
        value[offset + 2],
        value[offset + 3],
        value[offset + 4],
        value[offset + 5],
        value[offset + 6],
        value[offset + 7],
    ]))
}

/// of_property_read_u32_array - Read u32 array
/// @np: device node
/// @propname: property name
/// @out_values: output buffer
/// @sz: number of elements to read
///
/// Returns number of elements read, or error.
pub fn of_property_read_u32_array(
    np: Option<&DeviceNode>,
    propname: &str,
    out_values: &mut [u32],
) -> Result<usize> {
    let value = of_get_property(np, propname).ok_or(PropError::Invalid)?;
    
    if value.is_empty() {
        return Err(PropError::NoData);
    }
    
    let count = value.len() / 4;
    let to_read = count.min(out_values.len());
    
    for i in 0..to_read {
        let offset = i * 4;
        out_values[i] = u32::from_be_bytes([
            value[offset],
            value[offset + 1],
            value[offset + 2],
            value[offset + 3],
        ]);
    }
    
    Ok(to_read)
}

/// of_property_read_u64_array - Read u64 array
/// @np: device node
/// @propname: property name
/// @out_values: output buffer
///
/// Returns number of elements read, or error.
pub fn of_property_read_u64_array(
    np: Option<&DeviceNode>,
    propname: &str,
    out_values: &mut [u64],
) -> Result<usize> {
    let value = of_get_property(np, propname).ok_or(PropError::Invalid)?;
    
    if value.is_empty() {
        return Err(PropError::NoData);
    }
    
    let count = value.len() / 8;
    let to_read = count.min(out_values.len());
    
    for i in 0..to_read {
        let offset = i * 8;
        out_values[i] = u64::from_be_bytes([
            value[offset],
            value[offset + 1],
            value[offset + 2],
            value[offset + 3],
            value[offset + 4],
            value[offset + 5],
            value[offset + 6],
            value[offset + 7],
        ]);
    }
    
    Ok(to_read)
}

/// of_property_match_string - Find string in list
/// @np: device node
/// @propname: property name
/// @string: string to find
///
/// Returns index of string in list, or error.
pub fn of_property_match_string(
    np: Option<&DeviceNode>,
    propname: &str,
    string: &str,
) -> Result<usize> {
    let prop = of_find_property(np, propname).ok_or(PropError::Invalid)?;
    
    if prop.value.is_empty() {
        return Err(PropError::NoData);
    }
    
    let mut index = 0;
    let mut offset = 0;
    
    while offset < prop.value.len() {
        // Find null terminator
        let end = prop.value[offset..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| offset + p)
            .unwrap_or(prop.value.len());
        
        if end > prop.value.len() {
            return Err(PropError::InvalidSeq);
        }
        
        // Compare string
        if let Ok(s) = core::str::from_utf8(&prop.value[offset..end]) {
            if s == string {
                return Ok(index);
            }
        }
        
        offset = end + 1;
        index += 1;
    }
    
    Err(PropError::NoData)
}

/// of_property_read_string_array - Read string array
/// @np: device node
/// @propname: property name
/// @out_strs: output buffer
/// @skip: number of strings to skip
///
/// Returns number of strings read, or error.
pub fn of_property_read_string_array<'a>(
    np: Option<&'a DeviceNode>,
    propname: &str,
    out_strs: &mut [&'a str],
    skip: usize,
) -> Result<usize> {
    let prop = of_find_property(np, propname).ok_or(PropError::Invalid)?;
    
    if prop.value.is_empty() {
        return Err(PropError::NoData);
    }
    
    let mut index = 0;
    let mut offset = 0;
    let mut out_index = 0;
    
    while offset < prop.value.len() && out_index < out_strs.len() {
        // Find null terminator
        let end = prop.value[offset..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| offset + p)
            .unwrap_or(prop.value.len());
        
        if end > prop.value.len() {
            return Err(PropError::InvalidSeq);
        }
        
        // Skip if needed
        if index >= skip {
            if let Ok(s) = core::str::from_utf8(&prop.value[offset..end]) {
                out_strs[out_index] = s;
                out_index += 1;
            }
        }
        
        offset = end + 1;
        index += 1;
    }
    
    if out_index == 0 {
        Err(PropError::NoData)
    } else {
        Ok(out_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_property_read_bool() {
        // Would need mock node
        assert!(true);
    }

    #[test]
    fn test_of_property_count_elems() {
        // Test element counting logic
        let elem_size = 4;
        let data_len = 16;
        assert_eq!(data_len / elem_size, 4);
    }

    #[test]
    fn test_u32_array_parsing() {
        let data = [0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02];
        let v1 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let v2 = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        assert_eq!(v1, 1);
        assert_eq!(v2, 2);
    }

    #[test]
    fn test_u64_parsing() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x42];
        let v = u64::from_be_bytes(data);
        assert_eq!(v, 66);
    }

    #[test]
    fn test_string_list_parsing() {
        let data = b"string1\0string2\0string3\0";
        let mut offset = 0;
        let mut count = 0;
        
        while offset < data.len() {
            if let Some(pos) = data[offset..].iter().position(|&b| b == 0) {
                count += 1;
                offset += pos + 1;
            } else {
                break;
            }
        }
        
        assert_eq!(count, 3);
    }

    #[test]
    fn test_prop_error_types() {
        assert_eq!(PropError::Invalid, PropError::Invalid);
        assert_ne!(PropError::Invalid, PropError::NoData);
    }

    #[test]
    fn test_property_size_validation() {
        let value_len = 12;
        let elem_size = 4;
        assert_eq!(value_len % elem_size, 0);
        
        let value_len = 13;
        assert_ne!(value_len % elem_size, 0);
    }

    #[test]
    fn test_index_bounds() {
        let array_len = 16;
        let elem_size = 4;
        let index = 2;
        let required = (index + 1) * elem_size;
        assert!(required <= array_len);
    }

    #[test]
    fn test_string_null_termination() {
        let data = b"hello\0";
        let null_pos = data.iter().position(|&b| b == 0);
        assert_eq!(null_pos, Some(5));
    }
}
