//! Virtual memory management for ARM64
//!
//! Implements 4-level page tables (48-bit virtual address space):
//! - Level 0: PGD (Page Global Directory)
//! - Level 1: PUD (Page Upper Directory)  
//! - Level 2: PMD (Page Middle Directory)
//! - Level 3: PTE (Page Table Entry)

#[cfg(not(feature = "std"))]
use core::fmt;
#[cfg(feature = "std")]
use std::fmt;

use super::{PhysAddr, PAGE_SIZE};

/// Virtual address (48-bit on ARM64)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(usize);

impl VirtAddr {
    pub const fn new(addr: usize) -> Self {
        Self(addr & 0xFFFF_FFFF_FFFF) // Mask to 48 bits
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }

    pub const fn is_aligned(&self) -> bool {
        self.0 % PAGE_SIZE == 0
    }

    pub const fn page_offset(&self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }

    /// Extract page table index for given level (0-3)
    /// Level 0: bits 39-47, Level 1: bits 30-38, Level 2: bits 21-29, Level 3: bits 12-20
    pub const fn table_index(&self, level: usize) -> usize {
        let shift = 12 + (3 - level) * 9;
        (self.0 >> shift) & 0x1FF
    }

    pub const fn align_down(&self) -> Self {
        Self(self.0 & !(PAGE_SIZE - 1))
    }

    pub const fn align_up(&self) -> Self {
        Self((self.0 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
    }
}

/// Memory access flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags(u64);

impl Flags {
    pub const NONE: Self = Self(0);
    pub const VALID: Self = Self(1 << 0);      // Entry is valid
    pub const TABLE: Self = Self(1 << 1);      // Entry is a table (not block)
    pub const USER: Self = Self(1 << 6);       // User accessible
    pub const READ_ONLY: Self = Self(1 << 7);  // Read-only
    pub const ACCESSED: Self = Self(1 << 10);  // Accessed flag
    pub const DIRTY: Self = Self(1 << 51);     // Dirty flag (software)
    pub const COW: Self = Self(1 << 52);       // Copy-on-write (software)

    // Common combinations
    pub const KERNEL_RO: Self = Self(Self::VALID.0 | Self::READ_ONLY.0);
    pub const KERNEL_RW: Self = Self(Self::VALID.0);
    pub const USER_RO: Self = Self(Self::VALID.0 | Self::USER.0 | Self::READ_ONLY.0);
    pub const USER_RW: Self = Self(Self::VALID.0 | Self::USER.0);

    pub const fn new(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(&self) -> u64 {
        self.0
    }

    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn union(&self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn remove(&self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// Page table entry (ARM64 format)
#[derive(Clone, Copy)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const fn new() -> Self {
        Self(0)
    }

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(&self) -> u64 {
        self.0
    }

    pub const fn is_valid(&self) -> bool {
        (self.0 & Flags::VALID.0) != 0
    }

    pub const fn is_table(&self) -> bool {
        (self.0 & Flags::TABLE.0) != 0
    }

    pub const fn is_block(&self) -> bool {
        self.is_valid() && !self.is_table()
    }

    pub const fn flags(&self) -> Flags {
        Flags(self.0 & 0xFFF0_0000_0000_0FFF)
    }

    pub const fn phys_addr(&self) -> PhysAddr {
        PhysAddr::new((self.0 & 0x0000_FFFF_FFFF_F000) as usize)
    }

    pub fn set_addr(&mut self, addr: PhysAddr, flags: Flags) {
        self.0 = (addr.as_usize() as u64 & 0x0000_FFFF_FFFF_F000) | flags.0;
    }

    pub fn set_flags(&mut self, flags: Flags) {
        self.0 = (self.0 & 0x0000_FFFF_FFFF_F000) | flags.0;
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PageTableEntry")
            .field("valid", &self.is_valid())
            .field("table", &self.is_table())
            .field("addr", &format_args!("{:#x}", self.phys_addr().as_usize()))
            .field("flags", &format_args!("{:#x}", self.flags().bits()))
            .finish()
    }
}

/// Page table (512 entries, 4KB)
#[repr(align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::new(); 512],
        }
    }

    pub fn entry(&self, index: usize) -> Option<&PageTableEntry> {
        self.entries.get(index)
    }

    pub fn entry_mut(&mut self, index: usize) -> Option<&mut PageTableEntry> {
        self.entries.get_mut(index)
    }

    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            entry.clear();
        }
    }
}

impl fmt::Debug for PageTable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let valid_count = self.entries.iter().filter(|e| e.is_valid()).count();
        f.debug_struct("PageTable")
            .field("valid_entries", &valid_count)
            .finish()
    }
}

/// Virtual Memory Manager
pub struct VirtualMemory {
    root_table: PhysAddr,
    allocator: *const PhysicalAllocator,
}

impl VirtualMemory {
    /// Create new virtual memory manager
    /// 
    /// # Safety
    /// - root_table must point to valid 4KB-aligned page table
    /// - allocator must remain valid for lifetime of VirtualMemory
    pub unsafe fn new(root_table: PhysAddr, allocator: &PhysicalAllocator) -> Self {
        Self {
            root_table,
            allocator: allocator as *const _,
        }
    }

    /// Map virtual address to physical address with given flags
    pub fn map(&mut self, virt: VirtAddr, phys: PhysAddr, flags: Flags) -> Result<(), KernelError> {
        if !virt.is_aligned() || !phys.is_aligned() {
            return Err(KernelError::Memory(MemoryError::InvalidAlignment));
        }

        // Walk page table hierarchy, creating tables as needed
        let mut current_table_phys = self.root_table;

        for level in 0..3 {
            // SAFETY: current_table_phys points to valid page table (4KB aligned)
            // Initially root_table (validated in new()), then from valid PTEs
            let table = unsafe { &mut *(current_table_phys.as_usize() as *mut PageTable) };
            let index = virt.table_index(level);
            let entry = table.entry_mut(index).ok_or(KernelError::Memory(MemoryError::InvalidAddress))?;

            if !entry.is_valid() {
                // Allocate new page table
                // SAFETY: allocator pointer is valid (set in new(), immutable)
                let new_table_phys = unsafe { (*self.allocator).alloc_page()? };
                // SAFETY: new_table_phys is freshly allocated, valid 4KB page
                let new_table = unsafe { &mut *(new_table_phys.as_usize() as *mut PageTable) };
                
                // Clear new table
                new_table.clear();

                // Set entry to point to new table
                entry.set_addr(new_table_phys, Flags::TABLE.union(Flags::VALID));
            }

            current_table_phys = entry.phys_addr();
        }

        // Set final PTE
        // SAFETY: current_table_phys is valid (from loop above)
        let table = unsafe { &mut *(current_table_phys.as_usize() as *mut PageTable) };
        let index = virt.table_index(3);
        let entry = table.entry_mut(index).ok_or(KernelError::Memory(MemoryError::InvalidAddress))?;

        if entry.is_valid() {
            return Err(KernelError::Memory(MemoryError::AlreadyMapped));
        }

        entry.set_addr(phys, flags.union(Flags::VALID));

        Ok(())
    }

    /// Unmap virtual address
    pub fn unmap(&mut self, virt: VirtAddr) -> Result<PhysAddr, KernelError> {
        if !virt.is_aligned() {
            return Err(KernelError::Memory(MemoryError::InvalidAlignment));
        }

        // Walk page table hierarchy
        let mut current_table_phys = self.root_table;

        for level in 0..3 {
            // SAFETY: current_table_phys points to valid page table
            // Initially root_table, then from valid PTEs
            let table = unsafe { &*(current_table_phys.as_usize() as *const PageTable) };
            let index = virt.table_index(level);
            let entry = table.entry(index).ok_or(KernelError::Memory(MemoryError::InvalidAddress))?;

            if !entry.is_valid() {
                return Err(KernelError::Memory(MemoryError::NotMapped));
            }

            current_table_phys = entry.phys_addr();
        }

        // Clear final PTE
        // SAFETY: current_table_phys is valid (from loop above)
        let table = unsafe { &mut *(current_table_phys.as_usize() as *mut PageTable) };
        let index = virt.table_index(3);
        let entry = table.entry_mut(index).ok_or(KernelError::Memory(MemoryError::InvalidAddress))?;

        if !entry.is_valid() {
            return Err(KernelError::Memory(MemoryError::NotMapped));
        }

        let phys = entry.phys_addr();
        entry.clear();

        // TODO: Free intermediate page tables if empty
        // TODO: TLB invalidation

        Ok(phys)
    }

    /// Translate virtual address to physical address
    pub fn translate(&self, virt: VirtAddr) -> Result<PhysAddr, KernelError> {
        let aligned = virt.align_down();
        let offset = virt.page_offset();

        // Walk page table hierarchy
        let mut current_table_phys = self.root_table;

        for level in 0..4 {
            // SAFETY: current_table_phys points to valid page table
            // Initially root_table, then from valid PTEs
            let table = unsafe { &*(current_table_phys.as_usize() as *const PageTable) };
            let index = aligned.table_index(level);
            let entry = table.entry(index).ok_or(KernelError::Memory(MemoryError::InvalidAddress))?;

            if !entry.is_valid() {
                return Err(KernelError::Memory(MemoryError::NotMapped));
            }

            if level == 3 || entry.is_block() {
                // Found final mapping
                let phys_base = entry.phys_addr();
                return Ok(PhysAddr::new(phys_base.as_usize() + offset));
            }

            current_table_phys = entry.phys_addr();
        }

        Err(KernelError::Memory(MemoryError::NotMapped))
    }

    /// Change protection flags for mapped page
    pub fn protect(&mut self, virt: VirtAddr, flags: Flags) -> Result<(), KernelError> {
        if !virt.is_aligned() {
            return Err(KernelError::Memory(MemoryError::InvalidAlignment));
        }

        // Walk page table hierarchy
        let mut current_table_phys = self.root_table;

        for level in 0..3 {
            // SAFETY: current_table_phys points to valid page table
            let table = unsafe { &*(current_table_phys.as_usize() as *const PageTable) };
            let index = virt.table_index(level);
            let entry = table.entry(index).ok_or(KernelError::Memory(MemoryError::InvalidAddress))?;

            if !entry.is_valid() {
                return Err(KernelError::Memory(MemoryError::NotMapped));
            }

            current_table_phys = entry.phys_addr();
        }

        // Update final PTE flags
        // SAFETY: current_table_phys is valid (from loop above)
        let table = unsafe { &mut *(current_table_phys.as_usize() as *mut PageTable) };
        let index = virt.table_index(3);
        let entry = table.entry_mut(index).ok_or(KernelError::Memory(MemoryError::InvalidAddress))?;

        if !entry.is_valid() {
            return Err(KernelError::Memory(MemoryError::NotMapped));
        }

        entry.set_flags(flags.union(Flags::VALID));

        // TODO: TLB invalidation

        Ok(())
    }
}

use crate::error::{KernelError, MemoryError};
use super::PhysicalAllocator;

// SAFETY: VirtualMemory uses raw pointer to PhysicalAllocator but doesn't
// mutate it. All page table operations are synchronized externally.
unsafe impl Send for VirtualMemory {}
// SAFETY: All operations require &mut self for mutation, providing exclusive access
unsafe impl Sync for VirtualMemory {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virt_addr_table_index() {
        let addr = VirtAddr::new(0x0000_1234_5678_9ABC);
        
        // Level 0 (bits 39-47)
        assert_eq!(addr.table_index(0), 0x24);
        
        // Level 1 (bits 30-38)
        assert_eq!(addr.table_index(1), 0xD1);
        
        // Level 2 (bits 21-29)
        assert_eq!(addr.table_index(2), 0xB3);
        
        // Level 3 (bits 12-20)
        assert_eq!(addr.table_index(3), 0x189);
    }

    #[test]
    fn test_virt_addr_alignment() {
        assert!(VirtAddr::new(0x1000).is_aligned());
        assert!(!VirtAddr::new(0x1001).is_aligned());
        assert!(VirtAddr::new(0x0).is_aligned());
    }

    #[test]
    fn test_virt_addr_page_offset() {
        assert_eq!(VirtAddr::new(0x1234).page_offset(), 0x234);
        assert_eq!(VirtAddr::new(0x1000).page_offset(), 0);
        assert_eq!(VirtAddr::new(0x1FFF).page_offset(), 0xFFF);
    }

    #[test]
    fn test_virt_addr_align_down() {
        assert_eq!(VirtAddr::new(0x1234).align_down(), VirtAddr::new(0x1000));
        assert_eq!(VirtAddr::new(0x1000).align_down(), VirtAddr::new(0x1000));
        assert_eq!(VirtAddr::new(0x1FFF).align_down(), VirtAddr::new(0x1000));
    }

    #[test]
    fn test_virt_addr_align_up() {
        assert_eq!(VirtAddr::new(0x1234).align_up(), VirtAddr::new(0x2000));
        assert_eq!(VirtAddr::new(0x1000).align_up(), VirtAddr::new(0x1000));
        assert_eq!(VirtAddr::new(0x1001).align_up(), VirtAddr::new(0x2000));
    }

    #[test]
    fn test_flags() {
        let flags = Flags::KERNEL_RW.union(Flags::ACCESSED);
        assert!(flags.contains(Flags::VALID));
        assert!(flags.contains(Flags::ACCESSED));
        assert!(!flags.contains(Flags::USER));
    }

    #[test]
    fn test_flags_combinations() {
        assert!(Flags::KERNEL_RO.contains(Flags::VALID));
        assert!(Flags::KERNEL_RO.contains(Flags::READ_ONLY));
        assert!(!Flags::KERNEL_RO.contains(Flags::USER));

        assert!(Flags::USER_RW.contains(Flags::VALID));
        assert!(Flags::USER_RW.contains(Flags::USER));
        assert!(!Flags::USER_RW.contains(Flags::READ_ONLY));
    }

    #[test]
    fn test_flags_remove() {
        let flags = Flags::KERNEL_RW.union(Flags::ACCESSED);
        let without_accessed = flags.remove(Flags::ACCESSED);
        
        assert!(flags.contains(Flags::ACCESSED));
        assert!(!without_accessed.contains(Flags::ACCESSED));
        assert!(without_accessed.contains(Flags::VALID));
    }

    #[test]
    fn test_page_table_entry() {
        let mut pte = PageTableEntry::new();
        assert!(!pte.is_valid());
        assert!(!pte.is_table());
        assert!(!pte.is_block());

        let addr = PhysAddr::new(0x8000_0000);
        pte.set_addr(addr, Flags::KERNEL_RW);
        
        assert!(pte.is_valid());
        assert_eq!(pte.phys_addr(), addr);
        assert!(pte.flags().contains(Flags::VALID));
    }

    #[test]
    fn test_page_table_entry_table_vs_block() {
        let mut pte = PageTableEntry::new();
        let addr = PhysAddr::new(0x8000_0000);
        
        // Table entry
        pte.set_addr(addr, Flags::TABLE.union(Flags::VALID));
        assert!(pte.is_valid());
        assert!(pte.is_table());
        assert!(!pte.is_block());

        // Block entry
        pte.set_addr(addr, Flags::VALID);
        assert!(pte.is_valid());
        assert!(!pte.is_table());
        assert!(pte.is_block());
    }

    #[test]
    fn test_page_table_entry_clear() {
        let mut pte = PageTableEntry::new();
        pte.set_addr(PhysAddr::new(0x8000_0000), Flags::KERNEL_RW);
        assert!(pte.is_valid());

        pte.clear();
        assert!(!pte.is_valid());
        assert_eq!(pte.raw(), 0);
    }

    #[test]
    fn test_page_table_entry_set_flags() {
        let mut pte = PageTableEntry::new();
        let addr = PhysAddr::new(0x8000_0000);
        
        pte.set_addr(addr, Flags::KERNEL_RW);
        assert!(!pte.flags().contains(Flags::READ_ONLY));

        pte.set_flags(Flags::KERNEL_RO);
        assert!(pte.flags().contains(Flags::READ_ONLY));
        assert_eq!(pte.phys_addr(), addr); // Address unchanged
    }

    #[test]
    fn test_page_table() {
        let mut table = PageTable::new();
        
        // All entries should be invalid initially
        for i in 0..512 {
            assert!(!table.entry(i).unwrap().is_valid());
        }

        // Set one entry
        let entry = table.entry_mut(0).unwrap();
        entry.set_addr(PhysAddr::new(0x8000_0000), Flags::KERNEL_RW);
        
        assert!(table.entry(0).unwrap().is_valid());
        assert!(!table.entry(1).unwrap().is_valid());
    }

    #[test]
    fn test_page_table_clear() {
        let mut table = PageTable::new();
        
        // Set some entries
        for i in 0..10 {
            table.entry_mut(i).unwrap().set_addr(
                PhysAddr::new(0x8000_0000 + i * 0x1000),
                Flags::KERNEL_RW
            );
        }

        table.clear();

        // All should be invalid
        for i in 0..512 {
            assert!(!table.entry(i).unwrap().is_valid());
        }
    }

    #[test]
    fn test_page_table_bounds() {
        let table = PageTable::new();
        
        assert!(table.entry(0).is_some());
        assert!(table.entry(511).is_some());
        assert!(table.entry(512).is_none());
    }
}
