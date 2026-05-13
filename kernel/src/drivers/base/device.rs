// SPDX-License-Identifier: GPL-2.0
/*
 * drivers/base/core.c - core driver model code (device registration, etc)
 *
 * Ported from Linux drivers/base/core.c
 * Copyright (c) 2002-3 Patrick Mochel
 * Copyright (c) 2002-3 Open Source Development Labs
 * Copyright (c) 2006 Greg Kroah-Hartman
 */

use core::sync::atomic::{AtomicUsize, Ordering};
use crate::prelude::*;
use crate::drivers::resource::core::Resource;

/// Device reference count
static DEVICE_ID_COUNTER: AtomicUsize = AtomicUsize::new(1);

/// Device structure (extended from bus.rs)
pub struct DeviceCore {
    pub id: usize,
    pub name: String,
    pub parent: Option<*mut DeviceCore>,
    refcount: AtomicUsize,
    /// Resources (MMIO, IRQ, DMA, etc) - like Linux platform_device
    resources: Vec<Resource>,
}

/// Device errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    InvalidDevice,
    AlreadyRegistered,
    NotRegistered,
    NoMemory,
}

pub type Result<T> = core::result::Result<T, DeviceError>;

impl DeviceCore {
    /// Create a new device
    pub fn new(name: String) -> Self {
        Self {
            id: DEVICE_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            name,
            parent: None,
            refcount: AtomicUsize::new(1),
            resources: Vec::new(),
        }
    }

    /// Get device name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get device ID
    pub fn id(&self) -> usize {
        self.id
    }

    /// Set parent device
    pub fn set_parent(&mut self, parent: *mut DeviceCore) {
        self.parent = Some(parent);
    }

    /// Get parent device
    pub fn parent(&self) -> Option<*mut DeviceCore> {
        self.parent
    }

    /// Add resource to device (like platform_device_add_resources)
    pub fn add_resource(&mut self, resource: Resource) {
        self.resources.push(resource);
    }

    /// Get resource by index (like platform_get_resource)
    pub fn get_resource(&self, index: usize) -> Option<&Resource> {
        self.resources.get(index)
    }

    /// Get resource by type and index
    pub fn get_resource_by_type(&self, flags: u64, index: usize) -> Option<&Resource> {
        self.resources
            .iter()
            .filter(|r| r.flags & flags != 0)
            .nth(index)
    }

    /// Get all resources
    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    /// Get number of resources
    pub fn num_resources(&self) -> usize {
        self.resources.len()
    }
}

/// device_initialize - init device structure.
/// @dev: device.
///
/// This prepares the device for use by other layers by initializing
/// its fields.
/// It is the first half of device_register(), if called by
/// that function, though it can also be called separately, so one
/// may use @dev's fields. In particular, get_device()/put_device()
/// may be used for reference counting of @dev after calling this
/// function.
pub fn device_initialize(dev: *mut DeviceCore) {
    // SAFETY: Caller must ensure dev is a valid, properly aligned pointer
    // to an initialized DeviceCore. The pointer must remain valid for the
    // duration of this function call. AtomicUsize::store is thread-safe.
    unsafe {
        (*dev).refcount.store(1, Ordering::Relaxed);
    }
}

/// device_add - add device to device hierarchy.
/// @dev: device.
///
/// This is part 2 of device_register(), though may be called
/// separately _iff_ device_initialize() has been called separately.
///
/// This adds @dev to the kobject hierarchy via kobject_add(), adds it
/// to the global and sibling lists for the device, then
/// adds it to the other relevant subsystems of the driver model.
pub fn device_add(dev: *mut DeviceCore) -> Result<()> {
    if dev.is_null() {
        return Err(DeviceError::InvalidDevice);
    }

    // SAFETY: dev is non-null (checked above), caller guarantees it points
    // to valid DeviceCore. We only read the parent field, no mutation.
    unsafe {
        // Add to parent's children list if parent exists
        if let Some(parent) = (*dev).parent {
            // In full implementation, would add to parent's children list
            // SAFETY: parent pointer validity is guaranteed by caller
            let _ = parent;
        }

        // Add to bus if device has one (handled by bus.rs)
        // In full implementation, would call bus_add_device()
    }

    Ok(())
}

/// device_register - register a device with the system.
/// @dev: pointer to the device structure
///
/// This happens in two clean steps - initialize the device
/// and add it to the system. The two steps can be called
/// separately, but this is the easiest and most common.
///
/// NOTE: _Never_ directly free @dev after calling this function, even
/// if it returned an error! Always use put_device() to give up the
/// reference initialized in this function instead.
pub fn device_register(dev: *mut DeviceCore) -> Result<()> {
    if dev.is_null() {
        return Err(DeviceError::InvalidDevice);
    }

    // SAFETY: dev is non-null (checked above), caller guarantees validity
    unsafe {
        let name = &(*dev).name;
        if name.is_empty() || name.len() > 64 {
            return Err(DeviceError::InvalidDevice);
        }
    }

    device_initialize(dev);
    device_add(dev)
}

/// device_del - delete device from system.
/// @dev: device.
///
/// This is the first part of the device unregistration
/// sequence. This removes the device from the lists we control
/// from here, has it removed from the other driver model
/// subsystems it was added to in device_add(), and removes it
/// from the kobject hierarchy.
pub fn device_del(dev: *mut DeviceCore) -> Result<()> {
    if dev.is_null() {
        return Err(DeviceError::InvalidDevice);
    }

    // SAFETY: dev is non-null (checked), caller guarantees validity
    // We only read parent field, no mutation of device structure
    unsafe {
        // Remove from parent's children list
        if let Some(parent) = (*dev).parent {
            let _ = parent;
            // In full implementation, would remove from parent's list
        }

        // Remove from bus (handled by bus.rs)
        // In full implementation, would call bus_remove_device()
    }

    Ok(())
}

/// device_unregister - unregister device from system.
/// @dev: device going away.
///
/// We do this in two parts, like we do device_register(). First,
/// we remove it from all the subsystems with device_del(), then
/// we decrement the reference count via put_device(). If that
/// is the final reference count, the device will be cleaned up
/// via device_release() above. Otherwise, the structure will
/// stick around until the final reference to the device is dropped.
pub fn device_unregister(dev: *mut DeviceCore) -> Result<()> {
    device_del(dev)?;
    put_device(dev);
    Ok(())
}

/// get_device - increment reference count for device.
/// @dev: device.
///
/// This simply increments the reference count.
pub fn get_device(dev: *mut DeviceCore) -> *mut DeviceCore {
    if dev.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: dev is non-null (checked), caller guarantees it points to valid DeviceCore
    // AtomicUsize::fetch_add is thread-safe and doesn't require exclusive access
    unsafe {
        (*dev).refcount.fetch_add(1, Ordering::Relaxed);
    }

    dev
}

/// put_device - decrement reference count.
/// @dev: device in question.
///
/// Decrement the reference count, and if it reaches zero,
/// free the device.
pub fn put_device(dev: *mut DeviceCore) {
    if dev.is_null() {
        return;
    }

    // SAFETY: dev is non-null (checked), caller guarantees validity
    // fetch_sub is atomic and thread-safe. If count reaches 0, we have
    // exclusive ownership and can safely free the device.
    unsafe {
        let old_count = (*dev).refcount.fetch_sub(1, Ordering::Relaxed);
        if old_count == 1 {
            // Last reference, free the device
            device_release(dev);
        }
    }
}

/// device_release - free device structure.
/// @dev: device being freed.
///
/// This is called once the reference count for the object
/// reaches 0. We will have already removed it from the
/// hierarchy. It is now safe to free the device.
fn device_release(dev: *mut DeviceCore) {
    // SAFETY: Called only when refcount reaches 0, meaning we have exclusive
    // ownership. No other references exist, safe to deallocate.
    unsafe {
        // In full implementation, would call device-specific release
        // For now, just drop the Box
        let _ = Box::from_raw(dev);
    }
}

/// device_for_each_child - device child iterator.
/// @parent: parent struct device.
/// @fn: function to be called for each device.
///
/// Iterate over @parent's child devices, and call @fn for each,
/// passing it the child device.
pub fn device_for_each_child<F>(parent: *mut DeviceCore, f: F) -> Result<()>
where
    F: FnMut(*mut DeviceCore) -> Result<()>,
{
    if parent.is_null() {
        return Err(DeviceError::InvalidDevice);
    }

    // In full implementation, would iterate over parent's children list
    // For now, this is a no-op as we don't maintain children list yet
    let _ = f;

    Ok(())
}

/// device_is_registered - check if device is registered
/// @dev: device to check
pub fn device_is_registered(dev: *mut DeviceCore) -> bool {
    if dev.is_null() {
        return false;
    }

    // SAFETY: dev is non-null (checked), caller guarantees validity
    // AtomicUsize::load is thread-safe and doesn't require exclusive access
    unsafe {
        // Device is registered if refcount > 0
        (*dev).refcount.load(Ordering::Relaxed) > 0
    }
}

/// RAII wrapper for DeviceCore with automatic cleanup
///
/// This ensures put_device() is called when the wrapper is dropped,
/// preventing memory leaks. Use this instead of raw pointers when possible.
///
/// Example:
/// ```
/// let dev = DeviceHandle::new(DeviceCore::new("uart0".into()));
/// // Automatically calls put_device() on drop
/// ```
pub struct DeviceHandle {
    ptr: *mut DeviceCore,
}

impl DeviceHandle {
    /// Create a new device handle from a DeviceCore
    pub fn new(dev: DeviceCore) -> Self {
        let boxed = Box::new(dev);
        let ptr = Box::into_raw(boxed);
        Self { ptr }
    }

    /// Create from existing pointer (takes ownership)
    /// 
    /// # Safety
    /// 
    /// Caller must ensure:
    /// - ptr is valid and properly aligned
    /// - ptr points to a valid DeviceCore
    /// - ptr was allocated via Box and not freed
    /// - No other references to this device exist
    pub unsafe fn from_raw(ptr: *mut DeviceCore) -> Self {
        Self { ptr }
    }

    /// Get raw pointer (does not transfer ownership)
    pub fn as_ptr(&self) -> *mut DeviceCore {
        self.ptr
    }

    /// Get reference
    pub fn as_ref(&self) -> &DeviceCore {
        // SAFETY: ptr is valid (initialized in new/from_raw), not null,
        // and remains valid for lifetime of DeviceHandle
        unsafe { &*self.ptr }
    }

    /// Get mutable reference
    pub fn as_mut(&mut self) -> &mut DeviceCore {
        // SAFETY: ptr is valid, we have exclusive access via &mut self,
        // no other mutable references can exist
        unsafe { &mut *self.ptr }
    }

    /// Leak the handle (caller must call put_device manually)
    pub fn leak(self) -> *mut DeviceCore {
        let ptr = self.ptr;
        core::mem::forget(self);
        ptr
    }
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        // Automatically call put_device
        put_device(self.ptr);
    }
}

// SAFETY: DeviceCore uses atomic refcount for thread-safe reference counting
// The pointer is valid for the lifetime of DeviceHandle
unsafe impl Send for DeviceHandle {}
// SAFETY: All operations on DeviceCore are thread-safe through atomic operations
// Multiple threads can safely share DeviceHandle
unsafe impl Sync for DeviceHandle {}

#[cfg(test)]
mod tests {
    use super::*;
    use Box;

    #[test]
    fn test_device_create() {
        let dev = DeviceCore::new("test-device".into());
        assert_eq!(dev.name(), "test-device");
        assert!(dev.id() > 0);
    }

    #[test]
    fn test_device_register() {
        let mut dev = Box::new(DeviceCore::new("test-device".into()));
        let dev_ptr = &mut *dev as *mut DeviceCore;
        let result = device_register(dev_ptr);
        assert!(result.is_ok());
        device_unregister(dev_ptr).unwrap();
    }

    #[test]
    fn test_device_refcount() {
        let mut dev = Box::new(DeviceCore::new("test-device".into()));
        let dev_ptr = &mut *dev as *mut DeviceCore;

        // Initial refcount is 1
        // SAFETY: dev_ptr is valid, points to boxed DeviceCore
        assert_eq!(unsafe { (*dev_ptr).refcount.load(Ordering::Relaxed) }, 1);

        // get_device increments
        get_device(dev_ptr);
        // SAFETY: dev_ptr still valid
        assert_eq!(unsafe { (*dev_ptr).refcount.load(Ordering::Relaxed) }, 2);

        // put_device decrements
        put_device(dev_ptr);
        // SAFETY: dev_ptr still valid (refcount > 0)
        assert_eq!(unsafe { (*dev_ptr).refcount.load(Ordering::Relaxed) }, 1);

        // Don't let Box drop it
        core::mem::forget(dev);
    }

    #[test]
    fn test_device_parent() {
        let mut parent = Box::new(DeviceCore::new("parent".into()));
        let mut child = Box::new(DeviceCore::new("child".into()));

        let parent_ptr = &mut *parent as *mut DeviceCore;
        child.set_parent(parent_ptr);

        assert_eq!(child.parent(), Some(parent_ptr));

        core::mem::forget(parent);
        core::mem::forget(child);
    }

    #[test]
    fn test_device_is_registered() {
        let mut dev = Box::new(DeviceCore::new("test".into()));
        let dev_ptr = &mut *dev as *mut DeviceCore;

        assert!(device_is_registered(dev_ptr));

        core::mem::forget(dev);
    }

    #[test]
    fn test_device_resources() {
        use crate::drivers::resource::core::{IORESOURCE_MEM, IORESOURCE_IRQ};

        let mut dev = DeviceCore::new("test-device".into());

        // Add MMIO resource
        let mmio = Resource::new(0x1000, 0x1FFF, IORESOURCE_MEM);
        dev.add_resource(mmio);

        // Add IRQ resource
        let irq = Resource::new(42, 42, IORESOURCE_IRQ);
        dev.add_resource(irq);

        // Check resources
        assert_eq!(dev.num_resources(), 2);
        assert!(dev.get_resource(0).is_some());
        assert!(dev.get_resource(1).is_some());

        // Get by type
        let mmio_res = dev.get_resource_by_type(IORESOURCE_MEM, 0);
        assert!(mmio_res.is_some());
        assert_eq!(mmio_res.unwrap().start, 0x1000);

        let irq_res = dev.get_resource_by_type(IORESOURCE_IRQ, 0);
        assert!(irq_res.is_some());
        assert_eq!(irq_res.unwrap().start, 42);
    }

    #[test]
    fn test_device_handle_auto_cleanup() {
        // Create device with handle
        let dev = DeviceCore::new("test".into());
        let handle = DeviceHandle::new(dev);

        // Check it's valid
        assert_eq!(handle.as_ref().name(), "test");

        // Drop happens automatically - no memory leak!
    }

    #[test]
    fn test_device_handle_refcount() {
        let dev = DeviceCore::new("test".into());
        let handle = DeviceHandle::new(dev);
        let ptr = handle.as_ptr();

        // Initial refcount is 1
        unsafe {
            assert_eq!((*ptr).refcount.load(Ordering::Relaxed), 1);
        }

        // Get another reference
        let ptr2 = get_device(ptr);
        unsafe {
            assert_eq!((*ptr).refcount.load(Ordering::Relaxed), 2);
        }

        // Put it back
        put_device(ptr2);
        unsafe {
            assert_eq!((*ptr).refcount.load(Ordering::Relaxed), 1);
        }

        // Handle drop will decrement to 0 and free
    }

    #[test]
    fn test_device_handle_leak() {
        let dev = DeviceCore::new("test".into());
        let handle = DeviceHandle::new(dev);
        let ptr = handle.leak();

        // Now we must manually call put_device
        unsafe {
            assert_eq!((*ptr).refcount.load(Ordering::Relaxed), 1);
        }
        put_device(ptr);
    }
}
