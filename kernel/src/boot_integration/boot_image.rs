use crate::error::{KernelError, Result};
use crate::prelude::*;

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]

#[cfg(feature = "std")]
use std::vec::Vec;

const BOOT_MAGIC: &[u8; 8] = b"ANDROID!";
const PAGE_SIZE: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BootImageVersion {
    V2,
    V3,
    V4,
}

#[repr(C, packed)]
struct BootImageHeaderV2 {
    magic: [u8; 8],
    kernel_size: u32,
    kernel_addr: u32,
    ramdisk_size: u32,
    ramdisk_addr: u32,
    second_size: u32,
    second_addr: u32,
    tags_addr: u32,
    page_size: u32,
    header_version: u32,
    os_version: u32,
    name: [u8; 16],
    cmdline: [u8; 512],
    id: [u32; 8],
    extra_cmdline: [u8; 1024],
    recovery_dtbo_size: u32,
    recovery_dtbo_offset: u64,
    header_size: u32,
    dtb_size: u32,
    dtb_addr: u64,
}

#[repr(C, packed)]
struct BootImageHeaderV3 {
    magic: [u8; 8],
    kernel_size: u32,
    ramdisk_size: u32,
    os_version: u32,
    header_size: u32,
    reserved: [u32; 4],
    header_version: u32,
    cmdline: [u8; 1536],
}

#[repr(C, packed)]
struct BootImageHeaderV4 {
    magic: [u8; 8],
    kernel_size: u32,
    ramdisk_size: u32,
    os_version: u32,
    header_size: u32,
    reserved: [u32; 4],
    header_version: u32,
    cmdline: [u8; 1536],
    signature_size: u32,
}

pub struct BootImage {
    kernel: Vec<u8>,
    dtb: Vec<u8>,
    ramdisk: Option<Vec<u8>>,
    cmdline: Vec<u8>,
    version: BootImageVersion,
}

impl BootImage {
    pub fn new(kernel: Vec<u8>, dtb: Vec<u8>, version: BootImageVersion) -> Self {
        Self {
            kernel,
            dtb,
            ramdisk: None,
            cmdline: Vec::new(),
            version,
        }
    }

    pub fn with_ramdisk(mut self, ramdisk: Vec<u8>) -> Self {
        self.ramdisk = Some(ramdisk);
        self
    }

    pub fn with_cmdline(mut self, cmdline: &str) -> Self {
        self.cmdline = cmdline.as_bytes().to_vec();
        self
    }

    pub fn pack(&self) -> Result<Vec<u8>> {
        const MAX_KERNEL_SIZE: usize = 64 * 1024 * 1024; // 64MB

        if self.kernel.is_empty() {
            return Err(KernelError::InvalidAddress);
        }
        if self.kernel.len() > MAX_KERNEL_SIZE {
            return Err(KernelError::InvalidAddress);
        }
        // page_size must be a power of 2
        if PAGE_SIZE == 0 || (PAGE_SIZE & (PAGE_SIZE - 1)) != 0 {
            return Err(KernelError::InvalidAddress);
        }

        match self.version {
            BootImageVersion::V2 => self.pack_v2(),
            BootImageVersion::V3 => self.pack_v3(),
            BootImageVersion::V4 => self.pack_v4(),
        }
    }

    fn pack_v2(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        
        let header = BootImageHeaderV2 {
            magic: *BOOT_MAGIC,
            kernel_size: self.kernel.len() as u32,
            kernel_addr: 0x80000000,
            ramdisk_size: self.ramdisk.as_ref().map(|r| r.len() as u32).unwrap_or(0),
            ramdisk_addr: 0x81000000,
            second_size: 0,
            second_addr: 0,
            tags_addr: 0x80000100,
            page_size: PAGE_SIZE as u32,
            header_version: 2,
            os_version: 0,
            name: [0; 16],
            cmdline: [0; 512],
            id: [0; 8],
            extra_cmdline: [0; 1024],
            recovery_dtbo_size: 0,
            recovery_dtbo_offset: 0,
            header_size: core::mem::size_of::<BootImageHeaderV2>() as u32,
            dtb_size: self.dtb.len() as u32,
            dtb_addr: 0x82000000,
        };

        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const _ as *const u8,
                core::mem::size_of::<BootImageHeaderV2>(),
            )
        };
        output.extend_from_slice(header_bytes);
        Self::align_to_page(&mut output);

        output.extend_from_slice(&self.kernel);
        Self::align_to_page(&mut output);

        if let Some(ref ramdisk) = self.ramdisk {
            output.extend_from_slice(ramdisk);
            Self::align_to_page(&mut output);
        }

        output.extend_from_slice(&self.dtb);
        Self::align_to_page(&mut output);

        Ok(output)
    }

    fn pack_v3(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        
        let mut header = BootImageHeaderV3 {
            magic: *BOOT_MAGIC,
            kernel_size: self.kernel.len() as u32,
            ramdisk_size: self.ramdisk.as_ref().map(|r| r.len() as u32).unwrap_or(0),
            os_version: 0,
            header_size: core::mem::size_of::<BootImageHeaderV3>() as u32,
            reserved: [0; 4],
            header_version: 3,
            cmdline: [0; 1536],
        };

        if !self.cmdline.is_empty() {
            let len = self.cmdline.len().min(1536);
            header.cmdline[..len].copy_from_slice(&self.cmdline[..len]);
        }

        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const _ as *const u8,
                core::mem::size_of::<BootImageHeaderV3>(),
            )
        };
        output.extend_from_slice(header_bytes);
        Self::align_to_page(&mut output);

        output.extend_from_slice(&self.kernel);
        Self::align_to_page(&mut output);

        if let Some(ref ramdisk) = self.ramdisk {
            output.extend_from_slice(ramdisk);
            Self::align_to_page(&mut output);
        }

        output.extend_from_slice(&self.dtb);
        Self::align_to_page(&mut output);

        Ok(output)
    }

    fn pack_v4(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        
        let mut header = BootImageHeaderV4 {
            magic: *BOOT_MAGIC,
            kernel_size: self.kernel.len() as u32,
            ramdisk_size: self.ramdisk.as_ref().map(|r| r.len() as u32).unwrap_or(0),
            os_version: 0,
            header_size: core::mem::size_of::<BootImageHeaderV4>() as u32,
            reserved: [0; 4],
            header_version: 4,
            cmdline: [0; 1536],
            signature_size: 0,
        };

        if !self.cmdline.is_empty() {
            let len = self.cmdline.len().min(1536);
            header.cmdline[..len].copy_from_slice(&self.cmdline[..len]);
        }

        let header_bytes = unsafe {
            core::slice::from_raw_parts(
                &header as *const _ as *const u8,
                core::mem::size_of::<BootImageHeaderV4>(),
            )
        };
        output.extend_from_slice(header_bytes);
        Self::align_to_page(&mut output);

        output.extend_from_slice(&self.kernel);
        Self::align_to_page(&mut output);

        if let Some(ref ramdisk) = self.ramdisk {
            output.extend_from_slice(ramdisk);
            Self::align_to_page(&mut output);
        }

        output.extend_from_slice(&self.dtb);
        Self::align_to_page(&mut output);

        Ok(output)
    }

    fn align_to_page(data: &mut Vec<u8>) {
        let remainder = data.len() % PAGE_SIZE;
        if remainder != 0 {
            let padding = PAGE_SIZE - remainder;
            data.resize(data.len() + padding, 0);
        }
    }

    pub fn kernel_size(&self) -> usize {
        self.kernel.len()
    }

    pub fn dtb_size(&self) -> usize {
        self.dtb.len()
    }

    pub fn total_size(&self) -> usize {
        let mut size = PAGE_SIZE; // Header
        size += Self::aligned_size(self.kernel.len());
        if let Some(ref ramdisk) = self.ramdisk {
            size += Self::aligned_size(ramdisk.len());
        }
        size += Self::aligned_size(self.dtb.len());
        size
    }

    fn aligned_size(size: usize) -> usize {
        ((size + PAGE_SIZE - 1) / PAGE_SIZE) * PAGE_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_image_v2() {
        let kernel = vec![0u8; 1024];
        let dtb = vec![0u8; 512];
        let img = BootImage::new(kernel, dtb, BootImageVersion::V2);
        let packed = img.pack().unwrap();
        assert!(packed.len() >= img.total_size());
        assert_eq!(&packed[0..8], BOOT_MAGIC);
    }

    #[test]
    fn test_boot_image_v3() {
        let kernel = vec![0u8; 1024];
        let dtb = vec![0u8; 512];
        let img = BootImage::new(kernel, dtb, BootImageVersion::V3);
        let packed = img.pack().unwrap();
        assert!(packed.len() >= img.total_size());
    }

    #[test]
    fn test_boot_image_v4() {
        let kernel = vec![0u8; 1024];
        let dtb = vec![0u8; 512];
        let img = BootImage::new(kernel, dtb, BootImageVersion::V4);
        let packed = img.pack().unwrap();
        assert!(packed.len() >= img.total_size());
    }

    #[test]
    fn test_with_ramdisk() {
        let kernel = vec![0u8; 1024];
        let dtb = vec![0u8; 512];
        let ramdisk = vec![0u8; 2048];
        let img = BootImage::new(kernel, dtb, BootImageVersion::V3)
            .with_ramdisk(ramdisk);
        assert!(img.ramdisk.is_some());
    }

    #[test]
    fn test_with_cmdline() {
        let kernel = vec![0u8; 1024];
        let dtb = vec![0u8; 512];
        let img = BootImage::new(kernel, dtb, BootImageVersion::V3)
            .with_cmdline("console=ttyMSM0,115200");
        assert!(!img.cmdline.is_empty());
    }

    #[test]
    fn test_aligned_size() {
        assert_eq!(BootImage::aligned_size(100), 2048);
        assert_eq!(BootImage::aligned_size(2048), 2048);
        assert_eq!(BootImage::aligned_size(2049), 4096);
    }
}
