use crate::error::{KernelError, Result};
use crate::prelude::*;

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]

#[cfg(feature = "std")]
use std::vec::Vec;

pub fn parse_reg(prop: &[u8], address_cells: usize, size_cells: usize) -> Result<Vec<(u64, u64)>> {
    let cell_size = 4;
    let entry_size = (address_cells + size_cells) * cell_size;
    
    if prop.len() % entry_size != 0 {
        return Err(KernelError::InvalidAddress);
    }
    
    let mut result = Vec::new();
    let mut offset = 0;
    
    while offset < prop.len() {
        let addr = parse_cells(&prop[offset..], address_cells)?;
        offset += address_cells * cell_size;
        
        let size = parse_cells(&prop[offset..], size_cells)?;
        offset += size_cells * cell_size;
        
        result.push((addr, size));
    }
    
    Ok(result)
}

pub fn parse_interrupts(prop: &[u8]) -> Result<Vec<u32>> {
    if prop.len() % 4 != 0 {
        return Err(KernelError::InvalidAddress);
    }
    
    let mut result = Vec::new();
    let mut offset = 0;
    
    while offset < prop.len() {
        if offset + 4 > prop.len() {
            break;
        }
        
        let value = u32::from_be_bytes([
            prop[offset],
            prop[offset + 1],
            prop[offset + 2],
            prop[offset + 3],
        ]);
        result.push(value);
        offset += 4;
    }
    
    Ok(result)
}

pub fn parse_string(prop: &[u8]) -> Result<&str> {
    if prop.is_empty() {
        return Err(KernelError::InvalidAddress);
    }
    
    let len = prop.iter().position(|&b| b == 0).unwrap_or(prop.len());
    
    core::str::from_utf8(&prop[..len])
        .map_err(|_| KernelError::InvalidAddress)
}

pub fn parse_stringlist(prop: &[u8]) -> Result<Vec<&str>> {
    let mut result = Vec::new();
    let mut start = 0;
    
    for (i, &byte) in prop.iter().enumerate() {
        if byte == 0 {
            if i > start {
                if let Ok(s) = core::str::from_utf8(&prop[start..i]) {
                    result.push(s);
                }
            }
            start = i + 1;
        }
    }
    
    if result.is_empty() {
        return Err(KernelError::InvalidAddress);
    }
    
    Ok(result)
}

pub fn parse_u32(prop: &[u8]) -> Result<u32> {
    if prop.len() < 4 {
        return Err(KernelError::InvalidAddress);
    }
    
    Ok(u32::from_be_bytes([prop[0], prop[1], prop[2], prop[3]]))
}

pub fn parse_u64(prop: &[u8]) -> Result<u64> {
    if prop.len() < 8 {
        return Err(KernelError::InvalidAddress);
    }
    
    Ok(u64::from_be_bytes([
        prop[0], prop[1], prop[2], prop[3],
        prop[4], prop[5], prop[6], prop[7],
    ]))
}

pub fn parse_u32_array(prop: &[u8]) -> Result<Vec<u32>> {
    if prop.len() % 4 != 0 {
        return Err(KernelError::InvalidAddress);
    }
    
    let mut result = Vec::new();
    let mut offset = 0;
    
    while offset + 4 <= prop.len() {
        let value = u32::from_be_bytes([
            prop[offset],
            prop[offset + 1],
            prop[offset + 2],
            prop[offset + 3],
        ]);
        result.push(value);
        offset += 4;
    }
    
    Ok(result)
}

pub fn parse_phandle(prop: &[u8]) -> Result<u32> {
    parse_u32(prop)
}

pub fn parse_bool(prop: &[u8]) -> bool {
    !prop.is_empty()
}

fn parse_cells(data: &[u8], cells: usize) -> Result<u64> {
    if cells == 0 || cells > 2 {
        return Err(KernelError::InvalidAddress);
    }
    
    if data.len() < cells * 4 {
        return Err(KernelError::InvalidAddress);
    }
    
    let mut result = 0u64;
    
    for i in 0..cells {
        let offset = i * 4;
        let cell = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        result = (result << 32) | cell as u64;
    }
    
    Ok(result)
}

pub fn parse_ranges(prop: &[u8], child_addr_cells: usize, addr_cells: usize, size_cells: usize) -> Result<Vec<(u64, u64, u64)>> {
    let entry_size = (child_addr_cells + addr_cells + size_cells) * 4;
    
    if prop.is_empty() {
        return Ok(Vec::new());
    }
    
    if prop.len() % entry_size != 0 {
        return Err(KernelError::InvalidAddress);
    }
    
    let mut result = Vec::new();
    let mut offset = 0;
    
    while offset < prop.len() {
        let child_addr = parse_cells(&prop[offset..], child_addr_cells)?;
        offset += child_addr_cells * 4;
        
        let parent_addr = parse_cells(&prop[offset..], addr_cells)?;
        offset += addr_cells * 4;
        
        let size = parse_cells(&prop[offset..], size_cells)?;
        offset += size_cells * 4;
        
        result.push((child_addr, parent_addr, size));
    }
    
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_u32() {
        let data = [0x00, 0x00, 0x12, 0x34];
        assert_eq!(parse_u32(&data).unwrap(), 0x1234);
    }

    #[test]
    fn test_parse_u64() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x12, 0x34, 0x56, 0x78];
        assert_eq!(parse_u64(&data).unwrap(), 0x12345678);
    }

    #[test]
    fn test_parse_string() {
        let data = b"test\0";
        assert_eq!(parse_string(data).unwrap(), "test");
    }

    #[test]
    fn test_parse_stringlist() {
        let data = b"foo\0bar\0baz\0";
        let result = parse_stringlist(data).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "foo");
        assert_eq!(result[1], "bar");
        assert_eq!(result[2], "baz");
    }

    #[test]
    fn test_parse_reg() {
        let data = [
            0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00,
        ];
        let result = parse_reg(&data, 2, 2).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], (0x40000000, 0x10000000));
    }

    #[test]
    fn test_parse_interrupts() {
        let data = [0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02];
        let result = parse_interrupts(&data).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], 1);
        assert_eq!(result[1], 2);
    }

    #[test]
    fn test_parse_bool() {
        assert!(parse_bool(&[1]));
        assert!(!parse_bool(&[]));
    }

    #[test]
    fn test_parse_u32_array() {
        let data = [0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02];
        let result = parse_u32_array(&data).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], 1);
        assert_eq!(result[1], 2);
    }

    #[test]
    fn test_parse_ranges() {
        let data = [
            0x00, 0x00, 0x00, 0x00,
            0x40, 0x00, 0x00, 0x00,
            0x10, 0x00, 0x00, 0x00,
        ];
        let result = parse_ranges(&data, 1, 1, 1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], (0, 0x40000000, 0x10000000));
    }
}
