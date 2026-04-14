use crate::error::{KernelError, Result};
use crate::prelude::*;

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]

#[cfg(feature = "std")]
use std::{string::String, vec::Vec};

const FDT_MAGIC: u32 = 0xd00dfeed;
const FDT_BEGIN_NODE: u32 = 0x00000001;
const FDT_END_NODE: u32 = 0x00000002;
const FDT_PROP: u32 = 0x00000003;
const FDT_NOP: u32 = 0x00000004;
const FDT_END: u32 = 0x00000009;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FdtHeader {
    pub magic: u32,
    pub totalsize: u32,
    pub off_dt_struct: u32,
    pub off_dt_strings: u32,
    pub off_mem_rsvmap: u32,
    pub version: u32,
    pub last_comp_version: u32,
    pub boot_cpuid_phys: u32,
    pub size_dt_strings: u32,
    pub size_dt_struct: u32,
}

pub struct FdtParser {
    base: usize,
    header: FdtHeader,
}

impl FdtParser {
    pub fn new(addr: usize) -> Result<Self> {
        if addr == 0 || addr & 0x3 != 0 {
            return Err(KernelError::InvalidAddress);
        }
        
        let header = unsafe { Self::parse_header(addr)? };
        
        if header.totalsize > 0x1000000 {
            return Err(KernelError::InvalidAddress);
        }
        
        Ok(Self { base: addr, header })
    }

    pub unsafe fn parse_header(addr: usize) -> Result<FdtHeader> {
        let magic = u32::from_be(core::ptr::read_volatile(addr as *const u32));
        if magic != FDT_MAGIC {
            return Err(KernelError::InvalidAddress);
        }

        let header = FdtHeader {
            magic,
            totalsize: u32::from_be(core::ptr::read_volatile((addr + 4) as *const u32)),
            off_dt_struct: u32::from_be(core::ptr::read_volatile((addr + 8) as *const u32)),
            off_dt_strings: u32::from_be(core::ptr::read_volatile((addr + 12) as *const u32)),
            off_mem_rsvmap: u32::from_be(core::ptr::read_volatile((addr + 16) as *const u32)),
            version: u32::from_be(core::ptr::read_volatile((addr + 20) as *const u32)),
            last_comp_version: u32::from_be(core::ptr::read_volatile((addr + 24) as *const u32)),
            boot_cpuid_phys: u32::from_be(core::ptr::read_volatile((addr + 28) as *const u32)),
            size_dt_strings: u32::from_be(core::ptr::read_volatile((addr + 32) as *const u32)),
            size_dt_struct: u32::from_be(core::ptr::read_volatile((addr + 36) as *const u32)),
        };

        if header.version < 16 || header.version > 17 {
            return Err(KernelError::InvalidAddress);
        }
        
        if header.off_dt_struct as usize >= header.totalsize as usize ||
           header.off_dt_strings as usize >= header.totalsize as usize {
            return Err(KernelError::InvalidAddress);
        }

        Ok(header)
    }

    pub fn walk_nodes<F>(&self, mut callback: F) -> Result<()>
    where
        F: FnMut(&str, usize, usize) -> Result<bool>,
    {
        let struct_start = self.base + self.header.off_dt_struct as usize;
        let struct_end = struct_start + self.header.size_dt_struct as usize;
        let mut offset = 0;
        let mut depth = 0;
        let mut path = String::new();

        while struct_start + offset < struct_end {
            let token = self.read_u32(struct_start + offset)?;
            offset += 4;

            match token {
                FDT_BEGIN_NODE => {
                    let name_start = struct_start + offset;
                    let name = self.read_string(name_start)?;
                    let name_len = name.len() + 1;
                    offset += (name_len + 3) & !3;

                    if !path.is_empty() && depth > 0 {
                        path.push('/');
                    }
                    path.push_str(name);
                    depth += 1;

                    if !callback(&path, struct_start + offset, depth)? {
                        return Ok(());
                    }
                }
                FDT_END_NODE => {
                    if depth == 0 {
                        return Err(KernelError::InvalidAddress);
                    }
                    depth -= 1;
                    
                    if let Some(pos) = path.rfind('/') {
                        path.truncate(pos);
                    } else {
                        path.clear();
                    }
                }
                FDT_PROP => {
                    let len = self.read_u32(struct_start + offset)?;
                    let _nameoff = self.read_u32(struct_start + offset + 4)?;
                    offset += 8 + ((len as usize + 3) & !3);
                }
                FDT_NOP => {}
                FDT_END => break,
                _ => return Err(KernelError::InvalidAddress),
            }
        }

        if depth != 0 {
            return Err(KernelError::InvalidAddress);
        }

        Ok(())
    }

    pub fn read_property(&self, node_path: &str, prop_name: &str) -> Option<&[u8]> {
        let struct_start = self.base + self.header.off_dt_struct as usize;
        let struct_end = struct_start + self.header.size_dt_struct as usize;
        let mut offset = 0;
        let mut current_path = String::new();
        let mut depth = 0;
        let mut in_target_node = false;

        while struct_start + offset < struct_end {
            let token = self.read_u32(struct_start + offset).ok()?;
            offset += 4;

            match token {
                FDT_BEGIN_NODE => {
                    let name = self.read_string(struct_start + offset).ok()?;
                    let name_len = name.len() + 1;
                    offset += (name_len + 3) & !3;

                    if !current_path.is_empty() && depth > 0 {
                        current_path.push('/');
                    }
                    current_path.push_str(name);
                    depth += 1;

                    in_target_node = current_path == node_path;
                }
                FDT_END_NODE => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    
                    if let Some(pos) = current_path.rfind('/') {
                        current_path.truncate(pos);
                    } else {
                        current_path.clear();
                    }
                    in_target_node = false;
                }
                FDT_PROP => {
                    let len = self.read_u32(struct_start + offset).ok()? as usize;
                    let nameoff = self.read_u32(struct_start + offset + 4).ok()? as usize;
                    let data_offset = offset + 8;

                    if in_target_node {
                        let strings_base = self.base + self.header.off_dt_strings as usize;
                        if let Ok(name) = self.read_string(strings_base + nameoff) {
                            if name == prop_name {
                                let data = unsafe {
                                    core::slice::from_raw_parts(
                                        (struct_start + data_offset) as *const u8,
                                        len,
                                    )
                                };
                                return Some(data);
                            }
                        }
                    }

                    offset = data_offset + ((len + 3) & !3);
                }
                FDT_NOP => {}
                FDT_END => break,
                _ => return None,
            }
        }

        None
    }

    pub fn get_all_properties(&self, node_path: &str) -> Vec<(String, Vec<u8>)> {
        let mut properties = Vec::new();
        let struct_start = self.base + self.header.off_dt_struct as usize;
        let struct_end = struct_start + self.header.size_dt_struct as usize;
        let mut offset = 0;
        let mut current_path = String::new();
        let mut depth = 0;
        let mut in_target_node = false;

        while struct_start + offset < struct_end {
            if let Ok(token) = self.read_u32(struct_start + offset) {
                offset += 4;

                match token {
                    FDT_BEGIN_NODE => {
                        if let Ok(name) = self.read_string(struct_start + offset) {
                            let name_len = name.len() + 1;
                            offset += (name_len + 3) & !3;

                            if !current_path.is_empty() && depth > 0 {
                                current_path.push('/');
                            }
                            current_path.push_str(name);
                            depth += 1;

                            in_target_node = current_path == node_path;
                        } else {
                            break;
                        }
                    }
                    FDT_END_NODE => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                        
                        if let Some(pos) = current_path.rfind('/') {
                            current_path.truncate(pos);
                        } else {
                            current_path.clear();
                        }
                        in_target_node = false;
                    }
                    FDT_PROP => {
                        if let (Ok(len), Ok(nameoff)) = (
                            self.read_u32(struct_start + offset),
                            self.read_u32(struct_start + offset + 4)
                        ) {
                            let data_offset = offset + 8;

                            if in_target_node {
                                let strings_base = self.base + self.header.off_dt_strings as usize;
                                if let Ok(name) = self.read_string(strings_base + nameoff as usize) {
                                    let data = unsafe {
                                        core::slice::from_raw_parts(
                                            (struct_start + data_offset) as *const u8,
                                            len as usize,
                                        )
                                    };
                                    properties.push((String::from(name), data.to_vec()));
                                }
                            }

                            offset = data_offset + ((len as usize + 3) & !3);
                        } else {
                            break;
                        }
                    }
                    FDT_NOP => {}
                    FDT_END => break,
                    _ => break,
                }
            } else {
                break;
            }
        }

        properties
    }

    fn read_u32(&self, addr: usize) -> Result<u32> {
        if addr < self.base || addr + 4 > self.base + self.header.totalsize as usize {
            return Err(KernelError::InvalidAddress);
        }
        
        Ok(u32::from_be(unsafe {
            core::ptr::read_volatile(addr as *const u32)
        }))
    }

    fn read_string(&self, addr: usize) -> Result<&str> {
        if addr < self.base {
            return Err(KernelError::InvalidAddress);
        }
        
        let mut len = 0;
        loop {
            if addr + len >= self.base + self.header.totalsize as usize {
                return Err(KernelError::InvalidAddress);
            }
            
            let byte = unsafe { core::ptr::read_volatile((addr + len) as *const u8) };
            if byte == 0 {
                break;
            }
            len += 1;
            if len > 256 {
                return Err(KernelError::InvalidAddress);
            }
        }

        let bytes = unsafe { core::slice::from_raw_parts(addr as *const u8, len) };
        core::str::from_utf8(bytes).map_err(|_| KernelError::InvalidAddress)
    }
    
    pub fn get_header(&self) -> &FdtHeader {
        &self.header
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fdt_magic() {
        assert_eq!(FDT_MAGIC, 0xd00dfeed);
    }

    #[test]
    fn test_parse_header_invalid_magic() {
        let fake_dtb = [0u32; 16];
        let result = unsafe { FdtParser::parse_header(fake_dtb.as_ptr() as usize) };
        assert!(result.is_err());
    }

    #[test]
    fn test_fdt_tokens() {
        assert_eq!(FDT_BEGIN_NODE, 0x00000001);
        assert_eq!(FDT_END_NODE, 0x00000002);
        assert_eq!(FDT_PROP, 0x00000003);
        assert_eq!(FDT_END, 0x00000009);
    }
}
