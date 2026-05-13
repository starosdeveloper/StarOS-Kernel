use crate::error::{KernelError, Result};
use crate::prelude::*;
use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]

#[cfg(feature = "std")]
use std::vec::Vec;

#[derive(Debug, Clone, Copy)]
pub struct PhysAddr(pub u64);

#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub start: PhysAddr,
    pub size: usize,
    pub writable: bool,
    pub name: &'static str,
}

static MEMORY_MAP_INITIALIZED: AtomicUsize = AtomicUsize::new(0);

pub struct MemoryMap {
    safe_regions: Vec<MemoryRegion>,
    forbidden_regions: Vec<MemoryRegion>,
}

impl MemoryMap {
    pub fn new() -> Self {
        Self {
            safe_regions: Vec::new(),
            forbidden_regions: Vec::new(),
        }
    }

    pub fn from_device_tree(fdt_addr: usize) -> Result<Self> {
        let mut map = Self::new();
        
        map.add_forbidden_region(PhysAddr(0x00000000), 0x00100000, "bootloader");
        map.add_forbidden_region(PhysAddr(0x86000000), 0x02000000, "secure_world");
        map.add_forbidden_region(PhysAddr(0x86200000), 0x02E00000, "trustzone");
        
        unsafe {
            let magic = u32::from_be(core::ptr::read_volatile(fdt_addr as *const u32));
            if magic != 0xd00dfeed {
                return Err(KernelError::InvalidAddress);
            }
            
            let off_dt_struct = u32::from_be(core::ptr::read_volatile((fdt_addr + 8) as *const u32));
            let struct_start = fdt_addr + off_dt_struct as usize;
            
            map.parse_memory_nodes(struct_start)?;
            map.parse_reserved_memory(struct_start)?;
        }
        
        MEMORY_MAP_INITIALIZED.store(1, Ordering::Release);
        Ok(map)
    }

    unsafe fn parse_memory_nodes(&mut self, struct_start: usize) -> Result<()> {
        let mut offset = 0;
        
        loop {
            let token = u32::from_be(core::ptr::read_volatile((struct_start + offset) as *const u32));
            offset += 4;
            
            match token {
                0x00000001 => { // FDT_BEGIN_NODE
                    let name_ptr = (struct_start + offset) as *const u8;
                    let mut name_len = 0;
                    while core::ptr::read_volatile(name_ptr.add(name_len)) != 0 {
                        name_len += 1;
                        if name_len > 256 { break; }
                    }
                    let name = core::slice::from_raw_parts(name_ptr, name_len);
                    offset += (name_len + 1 + 3) & !3;
                    
                    if name.starts_with(b"memory@") {
                        // Find and parse reg property inline
                        let saved_offset = offset;
                        loop {
                            let prop_token = u32::from_be(core::ptr::read_volatile((struct_start + offset) as *const u32));
                            offset += 4;
                            
                            match prop_token {
                                0x00000003 => { // FDT_PROP
                                    let len = u32::from_be(core::ptr::read_volatile((struct_start + offset) as *const u32)) as usize;
                                    let data_offset = offset + 8;
                                    let reg_data = core::slice::from_raw_parts((struct_start + data_offset) as *const u8, len);
                                    
                                    if len >= 16 {
                                        let addr = u64::from_be_bytes([
                                            reg_data[0], reg_data[1], reg_data[2], reg_data[3],
                                            reg_data[4], reg_data[5], reg_data[6], reg_data[7],
                                        ]);
                                        let size = u64::from_be_bytes([
                                            reg_data[8], reg_data[9], reg_data[10], reg_data[11],
                                            reg_data[12], reg_data[13], reg_data[14], reg_data[15],
                                        ]);
                                        
                                        if size > 0 && size < 0x100000000 {
                                            self.add_safe_region(PhysAddr(addr), size as usize, "ram");
                                        }
                                    }
                                    
                                    offset = data_offset + ((len + 3) & !3);
                                    break;
                                }
                                0x00000002 => { // FDT_END_NODE
                                    offset = saved_offset;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                0x00000009 => break, // FDT_END
                0x00000003 => { // FDT_PROP
                    let len = u32::from_be(core::ptr::read_volatile((struct_start + offset) as *const u32)) as usize;
                    offset += 8 + ((len + 3) & !3);
                }
                _ => {}
            }
        }
        
        Ok(())
    }

    unsafe fn find_reg_property(&self, struct_start: usize, offset: &mut usize) -> Option<&[u8]> {
        let saved_offset = *offset;
        
        loop {
            let token = u32::from_be(core::ptr::read_volatile((struct_start + *offset) as *const u32));
            *offset += 4;
            
            match token {
                0x00000003 => { // FDT_PROP
                    let len = u32::from_be(core::ptr::read_volatile((struct_start + *offset) as *const u32)) as usize;
                    let data_offset = *offset + 8;
                    *offset = data_offset + ((len + 3) & !3);
                    
                    return Some(core::slice::from_raw_parts((struct_start + data_offset) as *const u8, len));
                }
                0x00000002 => { // FDT_END_NODE
                    *offset = saved_offset;
                    return None;
                }
                _ => {}
            }
        }
    }

    fn parse_reg_property(&mut self, data: &[u8]) -> Result<()> {
        if data.len() < 16 {
            return Ok(());
        }
        
        let addr = u64::from_be_bytes([
            data[0], data[1], data[2], data[3],
            data[4], data[5], data[6], data[7],
        ]);
        let size = u64::from_be_bytes([
            data[8], data[9], data[10], data[11],
            data[12], data[13], data[14], data[15],
        ]);
        
        if size > 0 && size < 0x100000000 {
            self.add_safe_region(PhysAddr(addr), size as usize, "ram");
        }
        
        Ok(())
    }

    unsafe fn parse_reserved_memory(&mut self, struct_start: usize) -> Result<()> {
        let mut offset = 0;
        
        loop {
            let token = u32::from_be(core::ptr::read_volatile((struct_start + offset) as *const u32));
            offset += 4;
            
            match token {
                0x00000001 => {
                    let name_ptr = (struct_start + offset) as *const u8;
                    let mut name_len = 0;
                    while core::ptr::read_volatile(name_ptr.add(name_len)) != 0 {
                        name_len += 1;
                        if name_len > 256 { break; }
                    }
                    let name = core::slice::from_raw_parts(name_ptr, name_len);
                    offset += (name_len + 1 + 3) & !3;
                    
                    if name.starts_with(b"reserved-memory") {
                        // Parse reserved regions as forbidden
                    }
                }
                0x00000009 => break,
                0x00000003 => {
                    let len = u32::from_be(core::ptr::read_volatile((struct_start + offset) as *const u32)) as usize;
                    offset += 8 + ((len + 3) & !3);
                }
                _ => {}
            }
        }
        
        Ok(())
    }

    pub fn add_safe_region(&mut self, start: PhysAddr, size: usize, name: &'static str) {
        self.safe_regions.push(MemoryRegion {
            start,
            size,
            writable: true,
            name,
        });
    }

    pub fn add_forbidden_region(&mut self, start: PhysAddr, size: usize, name: &'static str) {
        self.forbidden_regions.push(MemoryRegion {
            start,
            size,
            writable: false,
            name,
        });
    }

    pub fn is_safe_to_write(&self, addr: PhysAddr) -> bool {
        for region in &self.forbidden_regions {
            if addr.0 >= region.start.0 && addr.0 < region.start.0 + region.size as u64 {
                return false;
            }
        }

        for region in &self.safe_regions {
            if region.writable 
                && addr.0 >= region.start.0 
                && addr.0 < region.start.0 + region.size as u64 {
                return true;
            }
        }

        false
    }

    pub fn validate_region(&self, addr: PhysAddr, size: usize) -> Result<()> {
        if size == 0 {
            return Err(KernelError::InvalidAddress);
        }

        // Overflow check: ensure base + size does not wrap around
        let end_addr = addr.0.checked_add(size as u64)
            .ok_or(KernelError::InvalidAddress)?;

        // Additional sanity: end must be greater than start (catches size=0 edge after cast)
        if end_addr <= addr.0 {
            return Err(KernelError::InvalidAddress);
        }

        let mut current = addr.0;
        while current < end_addr {
            if !self.is_safe_to_write(PhysAddr(current)) {
                return Err(KernelError::InvalidAddress);
            }
            current += 4096;
        }

        Ok(())
    }

    pub fn is_forbidden(&self, addr: PhysAddr) -> bool {
        for region in &self.forbidden_regions {
            if addr.0 >= region.start.0 && addr.0 < region.start.0 + region.size as u64 {
                return true;
            }
        }
        false
    }

    pub fn get_safe_regions(&self) -> &[MemoryRegion] {
        &self.safe_regions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forbidden_bootloader() {
        let map = MemoryMap::new();
        assert!(!map.is_safe_to_write(PhysAddr(0x00000000)));
        assert!(map.is_forbidden(PhysAddr(0x00000000)));
    }

    #[test]
    fn test_forbidden_pmic() {
        let map = MemoryMap::new();
        assert!(!map.is_safe_to_write(PhysAddr(0x0C000000)));
        assert!(map.is_forbidden(PhysAddr(0x0C000000)));
    }

    #[test]
    fn test_safe_main_ram() {
        let map = MemoryMap::new();
        assert!(map.is_safe_to_write(PhysAddr(0x40000000)));
        assert!(!map.is_forbidden(PhysAddr(0x40000000)));
    }

    #[test]
    fn test_validate_region_safe() {
        let map = MemoryMap::new();
        assert!(map.validate_region(PhysAddr(0x40000000), 0x1000).is_ok());
    }

    #[test]
    fn test_validate_region_forbidden() {
        let map = MemoryMap::new();
        assert!(map.validate_region(PhysAddr(0x00000000), 0x1000).is_err());
    }

    #[test]
    fn test_validate_region_overflow() {
        let map = MemoryMap::new();
        assert!(map.validate_region(PhysAddr(0xFFFFFFFFFFFFFF00), 0x1000).is_err());
    }

    #[test]
    fn test_validate_large_region() {
        let map = MemoryMap::new();
        assert!(map.validate_region(PhysAddr(0x40000000), 0x10000000).is_ok());
    }

    #[test]
    fn test_uart_writable() {
        let map = MemoryMap::new();
        assert!(map.is_safe_to_write(PhysAddr(0x0A84000)));
    }
}
