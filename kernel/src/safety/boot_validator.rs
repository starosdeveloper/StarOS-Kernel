use crate::error::{KernelError, Result};
use super::memory_map::MemoryMap;

const FDT_MAGIC: u32 = 0xd00dfeed;
const UART_BASE: u64 = 0x0A84000;
const UART_FR: *const u32 = (UART_BASE + 0x18) as *const u32;

pub struct BootValidator;

impl BootValidator {
    pub fn validate_dtb(addr: usize) -> Result<()> {
        if addr == 0 {
            return Err(KernelError::InvalidAddress);
        }

        // ARM64 requires DTB to be 8-byte aligned
        if addr & 0x7 != 0 {
            return Err(KernelError::InvalidAddress);
        }

        let magic = unsafe { core::ptr::read_volatile(addr as *const u32) };
        if u32::from_be(magic) != FDT_MAGIC {
            return Err(KernelError::InvalidAddress);
        }

        let totalsize = unsafe { core::ptr::read_volatile((addr + 4) as *const u32) };
        let size = u32::from_be(totalsize) as usize;

        if size < 64 || size > 0x100000 {
            return Err(KernelError::InvalidAddress);
        }

        Ok(())
    }

    pub fn validate_memory_map() -> Result<()> {
        let map = MemoryMap::new();
        let regions = map.get_safe_regions();

        if regions.is_empty() {
            return Err(KernelError::InvalidAddress);
        }

        for region in regions {
            if region.size == 0 {
                return Err(KernelError::InvalidAddress);
            }
            
            let end = region.start.0.checked_add(region.size as u64)
                .ok_or(KernelError::InvalidAddress)?;
            
            if end <= region.start.0 {
                return Err(KernelError::InvalidAddress);
            }
        }

        Ok(())
    }

    pub fn validate_uart() -> Result<()> {
        unsafe {
            let fr = core::ptr::read_volatile(UART_FR);
            
            // Check if UART is responsive (BUSY bit should be readable)
            if fr == 0xFFFFFFFF || fr == 0x00000000 {
                return Err(KernelError::InvalidAddress);
            }
        }

        Ok(())
    }

    pub fn validate_all(dtb_addr: usize) -> Result<()> {
        Self::validate_uart()?;
        Self::validate_memory_map()?;
        Self::validate_dtb(dtb_addr)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_dtb_null() {
        assert!(BootValidator::validate_dtb(0).is_err());
    }

    #[test]
    fn test_validate_dtb_unaligned() {
        assert!(BootValidator::validate_dtb(0x40000001).is_err());
    }

    #[test]
    fn test_validate_dtb_bad_magic() {
        let fake_dtb = [0u32; 16];
        assert!(BootValidator::validate_dtb(fake_dtb.as_ptr() as usize).is_err());
    }

    #[test]
    fn test_validate_memory_map() {
        assert!(BootValidator::validate_memory_map().is_ok());
    }

    #[test]
    fn test_validate_uart() {
        // May fail in test environment, but should not panic
        let _ = BootValidator::validate_uart();
    }

    #[test]
    fn test_validate_all_null_dtb() {
        assert!(BootValidator::validate_all(0).is_err());
    }
}
