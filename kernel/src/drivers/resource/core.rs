// SPDX-License-Identifier: GPL-2.0-only
/*
 * Arbitrary resource management.
 *
 * Ported from Linux kernel/resource.c
 * Copyright (C) 1999 Linus Torvalds
 * Copyright (C) 1999 Martin Mares <mj@ucw.cz>
 *
 * SAFETY NOTES:
 * - Uses &'static str instead of String for early boot safety (no heap required)
 * - RwLock for concurrent access (frequent reads, rare writes)
 * - Raw pointers for tree structure (manual memory management like Linux)
 * - Safe for use before heap allocator initialization
 */

use spin::RwLock;

/// Resource flags
pub const IORESOURCE_IO: u64 = 0x00000100;
pub const IORESOURCE_MEM: u64 = 0x00000200;
pub const IORESOURCE_IRQ: u64 = 0x00000400;
pub const IORESOURCE_DMA: u64 = 0x00000800;
pub const IORESOURCE_BUS: u64 = 0x00001000;
pub const IORESOURCE_BUSY: u64 = 0x80000000;
pub const IORESOURCE_EXCLUSIVE: u64 = 0x08000000;
pub const IORESOURCE_MUXED: u64 = 0x00000040;
pub const IORESOURCE_SYSTEM_RAM: u64 = 0x01000000;

/// Resource descriptors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceDesc {
    None = 0,
    DevicePrivateMemory = 1,
    SoftReserved = 2,
}

/// Resource structure
pub struct Resource {
    pub start: u64,
    pub end: u64,
    pub name: Option<&'static str>,
    pub flags: u64,
    pub desc: ResourceDesc,
    pub parent: Option<*mut Resource>,
    pub sibling: Option<*mut Resource>,
    pub child: Option<*mut Resource>,
}

impl Resource {
    /// Create a new resource
    pub const fn new(start: u64, end: u64, flags: u64) -> Self {
        Self {
            start,
            end,
            name: None,
            flags,
            desc: ResourceDesc::None,
            parent: None,
            sibling: None,
            child: None,
        }
    }

    /// Create a new resource with name
    pub const fn new_named(start: u64, end: u64, flags: u64, name: &'static str) -> Self {
        Self {
            start,
            end,
            name: Some(name),
            flags,
            desc: ResourceDesc::None,
            parent: None,
            sibling: None,
            child: None,
        }
    }

    /// Get resource size
    pub fn size(&self) -> u64 {
        if self.end >= self.start {
            self.end - self.start + 1
        } else {
            0
        }
    }

    /// Check if resource contains another
    pub fn contains(&self, other: &Resource) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    /// Check if resources overlap
    pub fn overlaps(&self, other: &Resource) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

unsafe impl Send for Resource {}
unsafe impl Sync for Resource {}

impl Clone for Resource {
    fn clone(&self) -> Self {
        Self {
            start: self.start,
            end: self.end,
            name: self.name,
            flags: self.flags,
            desc: self.desc,
            parent: None, // Don't clone tree pointers
            sibling: None,
            child: None,
        }
    }
}

/// Global resource trees
static IOPORT_RESOURCE: RwLock<Resource> = RwLock::new(Resource {
    start: 0,
    end: 0xFFFF,
    name: None,
    flags: IORESOURCE_IO,
    desc: ResourceDesc::None,
    parent: None,
    sibling: None,
    child: None,
});

static IOMEM_RESOURCE: RwLock<Resource> = RwLock::new(Resource {
    start: 0,
    end: u64::MAX,
    name: None,
    flags: IORESOURCE_MEM,
    desc: ResourceDesc::None,
    parent: None,
    sibling: None,
    child: None,
});

/// Resource errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceError {
    Busy,
    Invalid,
    NoDevice,
    Range,
}

pub type Result<T> = core::result::Result<T, ResourceError>;

/// Return the next node in pre-order tree traversal
fn next_resource(
    p: *mut Resource,
    skip_children: bool,
    subtree_root: *mut Resource,
) -> Option<*mut Resource> {
    unsafe {
        if !skip_children {
            if let Some(child) = (*p).child {
                return Some(child);
            }
        }

        let mut current = p;
        loop {
            if (*current).sibling.is_some() {
                return (*current).sibling;
            }
            if let Some(parent) = (*current).parent {
                if parent == subtree_root {
                    return None;
                }
                current = parent;
            } else {
                return None;
            }
        }
    }
}

/// __request_resource - Internal function to request a resource
///
/// Returns the conflict entry if you can't request it, NULL on success
fn __request_resource(root: *mut Resource, new: *mut Resource) -> Option<*mut Resource> {
    unsafe {
        let start = (*new).start;
        let end = (*new).end;

        if end < start {
            return Some(root);
        }
        if start < (*root).start {
            return Some(root);
        }
        if end > (*root).end {
            return Some(root);
        }

        let mut p = &mut (*root).child;
        loop {
            if let Some(tmp) = *p {
                if (*tmp).start > end {
                    (*new).sibling = Some(tmp);
                    *p = Some(new);
                    (*new).parent = Some(root);
                    return None;
                }
                p = &mut (*tmp).sibling;
                if (*tmp).end < start {
                    continue;
                }
                return Some(tmp);
            } else {
                (*new).sibling = None;
                *p = Some(new);
                (*new).parent = Some(root);
                return None;
            }
        }
    }
}

/// __release_resource - Internal function to release a resource
fn __release_resource(old: *mut Resource, release_child: bool) -> Result<()> {
    unsafe {
        if let Some(parent) = (*old).parent {
            let mut p = &mut (*parent).child;
            loop {
                if let Some(tmp) = *p {
                    if tmp == old {
                        if release_child || (*tmp).child.is_none() {
                            *p = (*tmp).sibling;
                        } else {
                            // Move children up
                            let mut chd = (*tmp).child.unwrap();
                            loop {
                                (*chd).parent = Some(parent);
                                if (*chd).sibling.is_none() {
                                    break;
                                }
                                chd = (*chd).sibling.unwrap();
                            }
                            *p = (*tmp).child;
                            (*chd).sibling = (*tmp).sibling;
                        }
                        (*old).parent = None;
                        return Ok(());
                    }
                    p = &mut (*tmp).sibling;
                } else {
                    break;
                }
            }
        }
        Err(ResourceError::Invalid)
    }
}

/// request_resource - request and reserve an I/O or memory resource
/// @root: root resource descriptor
/// @new: resource descriptor desired by caller
///
/// Returns 0 for success, negative error code on error.
pub fn request_resource(root: *mut Resource, new: *mut Resource) -> Result<()> {
    let conflict = __request_resource(root, new);
    if conflict.is_some() {
        Err(ResourceError::Busy)
    } else {
        Ok(())
    }
}

/// release_resource - release a previously reserved resource
/// @old: resource pointer
pub fn release_resource(old: *mut Resource) -> Result<()> {
    __release_resource(old, true)
}

/// Resource constraint for allocation
pub struct ResourceConstraint {
    pub min: u64,
    pub max: u64,
    pub align: u64,
}

impl ResourceConstraint {
    pub fn new(min: u64, max: u64, align: u64) -> Self {
        Self { min, max, align }
    }
}

/// Clip resource to min/max bounds
fn resource_clip(res: &mut Resource, min: u64, max: u64) {
    if res.start < min {
        res.start = min;
    }
    if res.end > max {
        res.end = max;
    }
}

/// Align value up
fn align_up(val: u64, align: u64) -> u64 {
    (val + align - 1) & !(align - 1)
}

/// __find_resource_space - Find empty space in the resource tree
fn __find_resource_space(
    root: *mut Resource,
    old: Option<*mut Resource>,
    new: *mut Resource,
    size: u64,
    constraint: &ResourceConstraint,
) -> Result<()> {
    unsafe {
        let mut this = (*root).child;
        let mut tmp_start = (*root).start;

        // Skip past allocated resource at start
        if let Some(first) = this {
            if (*first).start == (*root).start {
                tmp_start = if Some(first) == old {
                    (*first).start
                } else {
                    (*first).end + 1
                };
                this = (*first).sibling;
            }
        }

        loop {
            let tmp_end = if let Some(current) = this {
                if Some(current) == old {
                    (*current).end
                } else {
                    (*current).start.saturating_sub(1)
                }
            } else {
                (*root).end
            };

            if tmp_end >= tmp_start {
                let mut avail_start = tmp_start;
                let avail_end = tmp_end;

                // Apply constraints
                resource_clip(
                    &mut Resource {
                        start: avail_start,
                        end: avail_end,
                        name: None,
                        flags: 0,
                        desc: ResourceDesc::None,
                        parent: None,
                        sibling: None,
                        child: None,
                    },
                    constraint.min,
                    constraint.max,
                );

                avail_start = align_up(avail_start, constraint.align);

                if avail_start >= tmp_start && avail_start <= avail_end {
                    let alloc_start = avail_start;
                    let alloc_end = alloc_start + size - 1;

                    if alloc_start <= alloc_end && alloc_end <= avail_end {
                        (*new).start = alloc_start;
                        (*new).end = alloc_end;
                        return Ok(());
                    }
                }
            }

            if this.is_none() || (*this.unwrap()).end == (*root).end {
                break;
            }

            if Some(this.unwrap()) != old {
                tmp_start = (*this.unwrap()).end + 1;
            }
            this = (*this.unwrap()).sibling;
        }

        Err(ResourceError::Busy)
    }
}

/// allocate_resource - allocate empty slot in the resource tree
/// @root: root resource descriptor
/// @new: resource descriptor desired by caller
/// @size: requested resource region size
/// @constraint: range and alignment constraints
///
/// The resource will be reallocated with a new size if it was already allocated
pub fn allocate_resource(
    root: *mut Resource,
    new: *mut Resource,
    size: u64,
    constraint: &ResourceConstraint,
) -> Result<()> {
    unsafe {
        if (*new).parent.is_some() {
            // Resource already allocated, try reallocating
            return reallocate_resource(root, new, size, constraint);
        }

        __find_resource_space(root, None, new, size, constraint)?;
        __request_resource(root, new)
            .map(|_| Err(ResourceError::Busy))
            .unwrap_or(Ok(()))
    }
}

/// reallocate_resource - reallocate a resource with new size
fn reallocate_resource(
    root: *mut Resource,
    old: *mut Resource,
    newsize: u64,
    constraint: &ResourceConstraint,
) -> Result<()> {
    unsafe {
        let mut new_res = Resource::new((*old).start, (*old).end, (*old).flags);

        __find_resource_space(root, Some(old), &mut new_res, newsize, constraint)?;

        if new_res.contains(&*old) {
            (*old).start = new_res.start;
            (*old).end = new_res.end;
            return Ok(());
        }

        if (*old).child.is_some() {
            return Err(ResourceError::Busy);
        }

        if (*old).contains(&new_res) {
            (*old).start = new_res.start;
            (*old).end = new_res.end;
        } else {
            __release_resource(old, true)?;
            (*old).start = new_res.start;
            (*old).end = new_res.end;
            __request_resource(root, old)
                .map(|_| Err(ResourceError::Busy))
                .unwrap_or(Ok(()))?;
        }

        Ok(())
    }
}

/// lookup_resource - find an existing resource by start address
/// @root: root resource descriptor
/// @start: resource start address
///
/// Returns a pointer to the resource if found, None otherwise
pub fn lookup_resource(root: *mut Resource, start: u64) -> Option<*mut Resource> {
    unsafe {
        let mut res = (*root).child;
        while let Some(current) = res {
            if (*current).start == start {
                return Some(current);
            }
            res = (*current).sibling;
        }
        None
    }
}

/// __insert_resource - Insert a resource into the resource tree
///
/// This is the advanced "reparenting" algorithm from Linux.
/// When a new resource "covers" multiple existing resources, it becomes their parent.
/// This is essential for bus management (e.g., PCI bridges covering multiple devices).
///
/// Example:
/// ```text
/// Before:
///   Root [0x0000-0xFFFF]
///     ├─ Device1 [0x1000-0x1FFF]
///     └─ Device2 [0x2000-0x2FFF]
///
/// After inserting Bridge [0x1000-0x2FFF]:
///   Root [0x0000-0xFFFF]
///     └─ Bridge [0x1000-0x2FFF]
///          ├─ Device1 [0x1000-0x1FFF]
///          └─ Device2 [0x2000-0x2FFF]
/// ```
///
/// Returns None on success, conflicting resource on error
fn __insert_resource(parent: *mut Resource, new: *mut Resource) -> Option<*mut Resource> {
    unsafe {
        let mut current_parent = parent;

        loop {
            let first = __request_resource(current_parent, new);
            if first.is_none() {
                return None;
            }

            let first = first.unwrap();
            if first == current_parent {
                return Some(first);
            }
            if first == new {
                return Some(first); // Duplicated insertion
            }

            if (*first).start > (*new).start || (*first).end < (*new).end {
                break;
            }
            if (*first).start == (*new).start && (*first).end == (*new).end {
                break;
            }

            current_parent = first;
        }

        // Find last conflicting sibling
        let first = __request_resource(current_parent, new).unwrap();
        let mut next = first;
        loop {
            if (*next).start < (*new).start || (*next).end > (*new).end {
                return Some(next); // Partial overlap
            }
            if (*next).sibling.is_none() {
                break;
            }
            let next_sib = (*next).sibling.unwrap();
            if (*next_sib).start > (*new).end {
                break;
            }
            next = next_sib;
        }

        // Reparent conflicting resources
        (*new).parent = Some(current_parent);
        (*new).sibling = (*next).sibling;
        (*new).child = Some(first);

        (*next).sibling = None;
        let mut current = first;
        while !current.is_null() {
            (*current).parent = Some(new);
            if let Some(sib) = (*current).sibling {
                current = sib;
            } else {
                break;
            }
        }

        if (*current_parent).child == Some(first) {
            (*current_parent).child = Some(new);
        } else {
            let mut scan = (*current_parent).child.unwrap();
            while (*scan).sibling != Some(first) {
                scan = (*scan).sibling.unwrap();
            }
            (*scan).sibling = Some(new);
        }

        None
    }
}

/// insert_resource - Insert a resource in the resource tree
/// @parent: parent of the new resource
/// @new: new resource to insert
///
/// Returns 0 on success, -EBUSY if the resource can't be inserted.
pub fn insert_resource(parent: *mut Resource, new: *mut Resource) -> Result<()> {
    let conflict = __insert_resource(parent, new);
    if conflict.is_some() {
        Err(ResourceError::Busy)
    } else {
        Ok(())
    }
}

/// remove_resource - Remove a resource from the resource tree
/// @old: resource to remove
///
/// Returns 0 on success, error on failure.
pub fn remove_resource(old: *mut Resource) -> Result<()> {
    __release_resource(old, false)
}

/// adjust_resource - modify a resource's start and size
/// @res: resource to modify
/// @start: new start value
/// @size: new size
///
/// Returns 0 on success, error if it can't fit.
pub fn adjust_resource(res: *mut Resource, start: u64, size: u64) -> Result<()> {
    unsafe {
        let end = start + size - 1;

        if let Some(parent) = (*res).parent {
            if start < (*parent).start || end > (*parent).end {
                return Err(ResourceError::Busy);
            }

            if let Some(sibling) = (*res).sibling {
                if (*sibling).start <= end {
                    return Err(ResourceError::Busy);
                }
            }

            let mut tmp = (*parent).child;
            if tmp != Some(res) {
                while let Some(current) = tmp {
                    if (*current).sibling == Some(res) {
                        if start <= (*current).end {
                            return Err(ResourceError::Busy);
                        }
                        break;
                    }
                    tmp = (*current).sibling;
                }
            }
        }

        // Check children
        let mut child = (*res).child;
        while let Some(current) = child {
            if (*current).start < start || (*current).end > end {
                return Err(ResourceError::Busy);
            }
            child = (*current).sibling;
        }

        (*res).start = start;
        (*res).end = end;
        Ok(())
    }
}

/// Get ioport resource root
pub fn get_ioport_resource() -> &'static RwLock<Resource> {
    &IOPORT_RESOURCE
}

/// Get iomem resource root
pub fn get_iomem_resource() -> &'static RwLock<Resource> {
    &IOMEM_RESOURCE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_size() {
        let res = Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM);
        assert_eq!(res.size(), 0x1000);
    }

    #[test]
    fn test_resource_contains() {
        let parent = Resource::new(0x1000, 0x2000, IORESOURCE_MEM);
        let child = Resource::new(0x1500, 0x1800, IORESOURCE_MEM);
        assert!(parent.contains(&child));
        assert!(!child.contains(&parent));
    }

    #[test]
    fn test_resource_overlaps() {
        let res1 = Resource::new(0x1000, 0x2000, IORESOURCE_MEM);
        let res2 = Resource::new(0x1800, 0x2800, IORESOURCE_MEM);
        let res3 = Resource::new(0x3000, 0x4000, IORESOURCE_MEM);
        assert!(res1.overlaps(&res2));
        assert!(!res1.overlaps(&res3));
    }

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0x1001, 0x1000), 0x2000);
        assert_eq!(align_up(0x1000, 0x1000), 0x1000);
        assert_eq!(align_up(0x1234, 0x100), 0x1300);
    }

    #[test]
    fn test_resource_clip() {
        let mut res = Resource::new(0x500, 0x2500, IORESOURCE_MEM);
        resource_clip(&mut res, 0x1000, 0x2000);
        assert_eq!(res.start, 0x1000);
        assert_eq!(res.end, 0x2000);
    }

    #[test]
    fn test_resource_named() {
        let res = Resource::new_named(0x1000, 0x2000, IORESOURCE_MEM, "test-device");
        assert_eq!(res.name, Some("test-device"));
        assert_eq!(res.start, 0x1000);
    }

    #[test]
    fn test_resource_tree_basic() {
        // Test basic resource tree operations
        let mut root = Resource::new(0, 0xFFFF, IORESOURCE_MEM);
        let mut child1 = Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM);
        let mut child2 = Resource::new(0x2000, 0x2FFF, IORESOURCE_MEM);

        // Request resources
        let result1 = __request_resource(&mut root, &mut child1);
        assert!(result1.is_none()); // Success

        let result2 = __request_resource(&mut root, &mut child2);
        assert!(result2.is_none()); // Success

        // Verify tree structure
        unsafe {
            assert!(root.child.is_some());
            assert_eq!((*root.child.unwrap()).start, 0x1000);
        }
    }
}
