//! Memory management
//!
//! Production-ready memory allocator with physical and virtual memory support

pub mod physical;
pub mod virtual_mem;
pub mod heap;
pub mod tlb;
pub mod iommu;
pub mod kaslr;

pub use physical::{PhysicalAllocator, PhysAddr, PAGE_SIZE};
pub use virtual_mem::{VirtAddr, PageTable, PageTableEntry, Flags, VirtualMemory};
pub use heap::{HeapAllocator, BuddyAllocator, SlabAllocator};

use crate::error::KernelError;

#[cfg(not(feature = "std"))]
pub type MemoryResult<T> = core::result::Result<T, KernelError>;

#[cfg(feature = "std")]
pub type MemoryResult<T> = std::result::Result<T, KernelError>;

