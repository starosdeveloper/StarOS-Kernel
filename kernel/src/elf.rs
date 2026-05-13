//! ELF64 parser and validator for STAR OS kernel.
//! Parses ELF64 binaries for aarch64, extracts PT_LOAD segments for mapping.

use alloc::vec::Vec;
use core::mem;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EM_AARCH64: u16 = 0xB7;
const PT_LOAD: u32 = 1;
const USER_SPACE_LIMIT: u64 = 0x0000_8000_0000_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    TooShort,
    BadMagic,
    NotElf64,
    NotLittleEndian,
    NotAarch64,
    InvalidPhdr,
    SegmentOutOfRange,
    OverlappingSegments,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadSegment {
    pub vaddr: u64,
    pub memsz: u64,
    pub filesz: u64,
    pub offset: u64,
    pub flags: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElfInfo {
    pub entry_point: u64,
    pub segments: Vec<LoadSegment>,
}

#[repr(C)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

pub fn parse_elf64(data: &[u8]) -> Result<ElfInfo, ElfError> {
    if data.len() < mem::size_of::<Elf64Ehdr>() {
        return Err(ElfError::TooShort);
    }

    // SAFETY: We verified data is large enough for the header, and Elf64Ehdr is repr(C) with no padding requirements beyond alignment.
    // We use ptr::read_unaligned to avoid alignment issues.
    let ehdr: Elf64Ehdr = unsafe {
        core::ptr::read_unaligned(data.as_ptr() as *const Elf64Ehdr)
    };

    if ehdr.e_ident[0..4] != ELF_MAGIC {
        return Err(ElfError::BadMagic);
    }
    if ehdr.e_ident[4] != ELFCLASS64 {
        return Err(ElfError::NotElf64);
    }
    if ehdr.e_ident[5] != ELFDATA2LSB {
        return Err(ElfError::NotLittleEndian);
    }
    if ehdr.e_machine != EM_AARCH64 {
        return Err(ElfError::NotAarch64);
    }

    let ph_off = ehdr.e_phoff as usize;
    let ph_size = ehdr.e_phentsize as usize;
    let ph_num = ehdr.e_phnum as usize;
    let phdr_end = ph_off.checked_add(ph_size.checked_mul(ph_num).ok_or(ElfError::InvalidPhdr)?)
        .ok_or(ElfError::InvalidPhdr)?;

    if ph_size < mem::size_of::<Elf64Phdr>() || phdr_end > data.len() {
        return Err(ElfError::InvalidPhdr);
    }

    let mut segments = Vec::new();
    for i in 0..ph_num {
        let off = ph_off + i * ph_size;
        // SAFETY: We verified bounds above and use read_unaligned.
        let phdr: Elf64Phdr = unsafe {
            core::ptr::read_unaligned(data.as_ptr().add(off) as *const Elf64Phdr)
        };

        if phdr.p_type != PT_LOAD {
            continue;
        }

        let end = phdr.p_vaddr.checked_add(phdr.p_memsz).ok_or(ElfError::SegmentOutOfRange)?;
        if end > USER_SPACE_LIMIT {
            return Err(ElfError::SegmentOutOfRange);
        }
        if phdr.p_filesz > phdr.p_memsz {
            return Err(ElfError::InvalidPhdr);
        }

        segments.push(LoadSegment {
            vaddr: phdr.p_vaddr,
            memsz: phdr.p_memsz,
            filesz: phdr.p_filesz,
            offset: phdr.p_offset,
            flags: phdr.p_flags,
        });
    }

    // Check for overlapping segments
    for i in 0..segments.len() {
        for j in (i + 1)..segments.len() {
            let a = &segments[i];
            let b = &segments[j];
            let a_end = a.vaddr + a.memsz;
            let b_end = b.vaddr + b.memsz;
            if a.vaddr < b_end && b.vaddr < a_end {
                return Err(ElfError::OverlappingSegments);
            }
        }
    }

    Ok(ElfInfo {
        entry_point: ehdr.e_entry,
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_elf(entry: u64, segments: &[(u64, u64, u64, u64, u32)]) -> Vec<u8> {
        let ehdr_size = mem::size_of::<Elf64Ehdr>();
        let phdr_size = mem::size_of::<Elf64Phdr>();
        let total = ehdr_size + phdr_size * segments.len();
        let mut buf = vec![0u8; total];

        // ELF header
        buf[0..4].copy_from_slice(&ELF_MAGIC);
        buf[4] = ELFCLASS64; // class
        buf[5] = ELFDATA2LSB; // endianness
        buf[6] = 1; // version
        // e_type at offset 16
        buf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        buf[18..20].copy_from_slice(&EM_AARCH64.to_le_bytes());
        buf[20..24].copy_from_slice(&1u32.to_le_bytes()); // version
        buf[24..32].copy_from_slice(&entry.to_le_bytes());
        buf[32..40].copy_from_slice(&(ehdr_size as u64).to_le_bytes()); // e_phoff
        // e_phentsize at offset 54
        buf[54..56].copy_from_slice(&(phdr_size as u16).to_le_bytes());
        buf[56..58].copy_from_slice(&(segments.len() as u16).to_le_bytes());

        // Program headers
        for (i, &(vaddr, memsz, filesz, offset, flags)) in segments.iter().enumerate() {
            let base = ehdr_size + i * phdr_size;
            buf[base..base + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
            buf[base + 4..base + 8].copy_from_slice(&flags.to_le_bytes());
            buf[base + 8..base + 16].copy_from_slice(&offset.to_le_bytes());
            buf[base + 16..base + 24].copy_from_slice(&vaddr.to_le_bytes());
            buf[base + 24..base + 32].copy_from_slice(&vaddr.to_le_bytes()); // paddr
            buf[base + 32..base + 40].copy_from_slice(&filesz.to_le_bytes());
            buf[base + 40..base + 48].copy_from_slice(&memsz.to_le_bytes());
        }
        buf
    }

    #[test]
    fn test_valid_elf() {
        let data = make_elf(0x400000, &[(0x400000, 0x1000, 0x800, 0, 5)]);
        let info = parse_elf64(&data).unwrap();
        assert_eq!(info.entry_point, 0x400000);
        assert_eq!(info.segments.len(), 1);
        assert_eq!(info.segments[0].vaddr, 0x400000);
        assert_eq!(info.segments[0].memsz, 0x1000);
        assert_eq!(info.segments[0].filesz, 0x800);
    }

    #[test]
    fn test_bad_magic() {
        let mut data = make_elf(0x400000, &[(0x400000, 0x1000, 0x800, 0, 5)]);
        data[0] = 0;
        assert_eq!(parse_elf64(&data), Err(ElfError::BadMagic));
    }

    #[test]
    fn test_too_short() {
        assert_eq!(parse_elf64(&[0; 4]), Err(ElfError::TooShort));
    }

    #[test]
    fn test_segment_out_of_range() {
        let data = make_elf(0x400000, &[(USER_SPACE_LIMIT, 0x1000, 0x800, 0, 5)]);
        assert_eq!(parse_elf64(&data), Err(ElfError::SegmentOutOfRange));
    }

    #[test]
    fn test_overlapping_segments() {
        let data = make_elf(0x400000, &[
            (0x400000, 0x2000, 0x1000, 0, 5),
            (0x401000, 0x1000, 0x800, 0, 6),
        ]);
        assert_eq!(parse_elf64(&data), Err(ElfError::OverlappingSegments));
    }

    #[test]
    fn test_non_overlapping_segments() {
        let data = make_elf(0x400000, &[
            (0x400000, 0x1000, 0x800, 0, 5),
            (0x500000, 0x2000, 0x1000, 0, 6),
        ]);
        let info = parse_elf64(&data).unwrap();
        assert_eq!(info.segments.len(), 2);
    }
}
