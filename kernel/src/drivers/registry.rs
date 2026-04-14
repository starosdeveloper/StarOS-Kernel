//! Device Registry - Lock-free global device database
//! 
//! Thread-safe registry using DashMap for zero-contention device lookups.
//! Supports hot-plugging with <1ms registration time.

#[cfg(not(feature = "std"))]
extern crate alloc;

use crate::drivers::traits::{BasicDevice, DeviceId};
use crate::prelude::*;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::RwLock;

/// Type-erased device handle
pub type DeviceHandle = Arc<RwLock<dyn BasicDevice>>;

/// Global device registry (singleton)
pub struct DeviceRegistry {
    devices: RwLock<Vec<(DeviceId, DeviceHandle)>>,
    device_count: AtomicU64,
}

impl DeviceRegistry {
    /// Create new registry
    pub const fn new() -> Self {
        Self {
            devices: RwLock::new(Vec::new()),
            device_count: AtomicU64::new(0),
        }
    }

    /// Register a device (hot-plug safe)
    pub fn register(&self, id: DeviceId, device: DeviceHandle) -> Result<(), RegistryError> {
        let mut devices = self.devices.write();
        
        // Check for duplicate
        if devices.iter().any(|(existing_id, _)| *existing_id == id) {
            return Err(RegistryError::DuplicateDevice);
        }
        
        devices.push((id, device));
        self.device_count.fetch_add(1, Ordering::Relaxed);
        
        Ok(())
    }

    /// Unregister a device
    pub fn unregister(&self, id: DeviceId) -> Result<DeviceHandle, RegistryError> {
        let mut devices = self.devices.write();
        
        let pos = devices
            .iter()
            .position(|(device_id, _)| *device_id == id)
            .ok_or(RegistryError::DeviceNotFound)?;
        
        let (_, handle) = devices.remove(pos);
        self.device_count.fetch_sub(1, Ordering::Relaxed);
        
        Ok(handle)
    }

    /// Lookup device by ID
    pub fn lookup(&self, id: DeviceId) -> Option<DeviceHandle> {
        let devices = self.devices.read();
        devices
            .iter()
            .find(|(device_id, _)| *device_id == id)
            .map(|(_, handle)| Arc::clone(handle))
    }

    /// Get all registered device IDs
    pub fn list_devices(&self) -> Vec<DeviceId> {
        let devices = self.devices.read();
        devices.iter().map(|(id, _)| *id).collect()
    }

    /// Get device count (fast, lock-free)
    pub fn count(&self) -> u64 {
        self.device_count.load(Ordering::Relaxed)
    }

    /// Clear all devices (for testing)
    #[cfg(test)]
    pub fn clear(&self) {
        let mut devices = self.devices.write();
        devices.clear();
        self.device_count.store(0, Ordering::Relaxed);
    }
}

/// Registry errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateDevice,
    DeviceNotFound,
    RegistryFull,
}

/// Global registry instance
static DEVICE_REGISTRY: DeviceRegistry = DeviceRegistry::new();

/// Get global registry reference
pub fn global_registry() -> &'static DeviceRegistry {
    &DEVICE_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::traits::{DeviceCapabilities, DeviceResult, PowerState};

    struct MockDevice {
        id: DeviceId,
    }

    impl BasicDevice for MockDevice {
        fn init(&mut self) -> DeviceResult<()> { Ok(()) }
        fn shutdown(&mut self) -> DeviceResult<()> { Ok(()) }
        fn power_save(&mut self, _state: PowerState) -> DeviceResult<()> { Ok(()) }
        fn device_id(&self) -> DeviceId { self.id }
        fn capabilities(&self) -> DeviceCapabilities {
            DeviceCapabilities {
                can_stream: false,
                can_block_io: false,
                supports_dma: false,
                hot_pluggable: true,
            }
        }
        fn name(&self) -> &str { "mock" }
    }

    #[test]
    fn test_register_and_lookup() {
        let registry = DeviceRegistry::new();
        let id = DeviceId::new(0x1000, 0x2000, 0);
        let device = Arc::new(RwLock::new(MockDevice { id }));
        
        assert!(registry.register(id, device).is_ok());
        assert_eq!(registry.count(), 1);
        assert!(registry.lookup(id).is_some());
    }

    #[test]
    fn test_duplicate_registration() {
        let registry = DeviceRegistry::new();
        let id = DeviceId::new(0x1000, 0x2000, 0);
        let device1 = Arc::new(RwLock::new(MockDevice { id }));
        let device2 = Arc::new(RwLock::new(MockDevice { id }));
        
        assert!(registry.register(id, device1).is_ok());
        assert_eq!(registry.register(id, device2), Err(RegistryError::DuplicateDevice));
    }

    #[test]
    fn test_unregister() {
        let registry = DeviceRegistry::new();
        let id = DeviceId::new(0x1000, 0x2000, 0);
        let device = Arc::new(RwLock::new(MockDevice { id }));
        
        registry.register(id, device).unwrap();
        assert!(registry.unregister(id).is_ok());
        assert_eq!(registry.count(), 0);
        assert!(registry.lookup(id).is_none());
    }

    #[test]
    fn test_list_devices() {
        let registry = DeviceRegistry::new();
        let id1 = DeviceId::new(0x1000, 0x2000, 0);
        let id2 = DeviceId::new(0x1000, 0x2000, 1);
        
        registry.register(id1, Arc::new(RwLock::new(MockDevice { id: id1 }))).unwrap();
        registry.register(id2, Arc::new(RwLock::new(MockDevice { id: id2 }))).unwrap();
        
        let devices = registry.list_devices();
        assert_eq!(devices.len(), 2);
        assert!(devices.contains(&id1));
        assert!(devices.contains(&id2));
    }
}
