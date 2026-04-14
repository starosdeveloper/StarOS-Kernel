//! Driver Manager - Linux-inspired automatic device-driver matching
//! 
//! Implements the core Linux driver model concepts in Rust:
//! - Automatic device ↔ driver matching
//! - Probe/remove lifecycle
//! - Hot-plug support
//! - Bus subsystem management

#[cfg(not(feature = "std"))]
extern crate alloc;

use crate::drivers::traits::{BasicDevice, DeviceId, DeviceCapabilities, DeviceResult, DeviceError};
use crate::prelude::*;
use crate::drivers::registry::{DeviceRegistry, DeviceHandle};
use crate::drivers::bus::{BusType, BusScanResult};
use spin::RwLock;

/// Probe strategy (from Linux probe_type)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeType {
    /// Synchronous probing (default)
    Synchronous,
    /// Asynchronous probing (for slow devices)
    Asynchronous,
    /// Force synchronous (critical devices)
    ForceSynchronous,
}

/// Extended device ID with subsystem info (like Linux pci_device_id)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExtendedDeviceId {
    pub vendor: u16,
    pub device: u16,
    pub subvendor: u16,
    pub subdevice: u16,
    pub class: u32,
    pub class_mask: u32,
}

impl ExtendedDeviceId {
    pub const ANY: u16 = 0xFFFF;
    pub const ANY_CLASS: u32 = 0xFFFFFFFF;
    
    pub fn new(vendor: u16, device: u16) -> Self {
        Self {
            vendor,
            device,
            subvendor: Self::ANY,
            subdevice: Self::ANY,
            class: Self::ANY_CLASS,
            class_mask: 0,
        }
    }
    
    pub fn with_subsystem(vendor: u16, device: u16, subvendor: u16, subdevice: u16) -> Self {
        Self {
            vendor,
            device,
            subvendor,
            subdevice,
            class: Self::ANY_CLASS,
            class_mask: 0,
        }
    }
    
    pub fn with_class(vendor: u16, device: u16, class: u32, class_mask: u32) -> Self {
        Self {
            vendor,
            device,
            subvendor: Self::ANY,
            subdevice: Self::ANY,
            class,
            class_mask,
        }
    }
    
    /// Check if this ID matches another (wildcard support)
    pub fn matches(&self, other: &ExtendedDeviceId) -> bool {
        // Vendor match
        if self.vendor != Self::ANY && other.vendor != Self::ANY {
            if self.vendor != other.vendor {
                return false;
            }
        }
        
        // Device match
        if self.device != Self::ANY && other.device != Self::ANY {
            if self.device != other.device {
                return false;
            }
        }
        
        // Subvendor match
        if self.subvendor != Self::ANY && other.subvendor != Self::ANY {
            if self.subvendor != other.subvendor {
                return false;
            }
        }
        
        // Subdevice match
        if self.subdevice != Self::ANY && other.subdevice != Self::ANY {
            if self.subdevice != other.subdevice {
                return false;
            }
        }
        
        // Class match (with mask)
        if self.class != Self::ANY_CLASS && other.class != Self::ANY_CLASS {
            if (self.class & self.class_mask) != (other.class & self.class_mask) {
                return false;
            }
        }
        
        true
    }
    
    pub fn to_simple(&self) -> DeviceId {
        DeviceId::new(self.vendor, self.device, 0)
    }
}

/// Driver descriptor (like Linux device_driver)
pub struct Driver {
    pub name: &'static str,
    pub bus_type: BusType,
    pub probe_type: ProbeType,
    pub id_table: Vec<ExtendedDeviceId>,
    pub probe: fn(&mut dyn BasicDevice) -> DeviceResult<()>,
    pub remove: fn(&mut dyn BasicDevice) -> DeviceResult<()>,
}

impl Driver {
    pub fn new(name: &'static str, bus_type: BusType) -> Self {
        Self {
            name,
            bus_type,
            probe_type: ProbeType::Synchronous,
            id_table: Vec::new(),
            probe: |dev| dev.init(),
            remove: |dev| dev.shutdown(),
        }
    }
    
    pub fn with_ids(mut self, ids: Vec<ExtendedDeviceId>) -> Self {
        self.id_table = ids;
        self
    }
    
    pub fn with_probe_type(mut self, probe_type: ProbeType) -> Self {
        self.probe_type = probe_type;
        self
    }
    
    pub fn with_probe(mut self, probe: fn(&mut dyn BasicDevice) -> DeviceResult<()>) -> Self {
        self.probe = probe;
        self
    }
    
    pub fn with_remove(mut self, remove: fn(&mut dyn BasicDevice) -> DeviceResult<()>) -> Self {
        self.remove = remove;
        self
    }
    
    /// Check if this driver supports a device
    pub fn matches(&self, device_id: &ExtendedDeviceId) -> bool {
        for id in &self.id_table {
            if id.matches(device_id) {
                return true;
            }
        }
        false
    }
}

/// Driver Manager - orchestrates device-driver matching (like Linux bus.c)
pub struct DriverManager {
    drivers: RwLock<Vec<Arc<Driver>>>,
    pending_devices: RwLock<Vec<(ExtendedDeviceId, DeviceHandle)>>,
}

impl DriverManager {
    pub const fn new() -> Self {
        Self {
            drivers: RwLock::new(Vec::new()),
            pending_devices: RwLock::new(Vec::new()),
        }
    }
    
    /// Register a driver (like driver_register in Linux)
    pub fn register_driver(&self, driver: Arc<Driver>) -> DeviceResult<()> {
        let mut drivers = self.drivers.write();
        drivers.push(driver.clone());
        
        // Try to match pending devices
        self.match_pending_devices(&driver);
        
        Ok(())
    }
    
    /// Unregister a driver
    pub fn unregister_driver(&self, name: &str) -> DeviceResult<()> {
        let mut drivers = self.drivers.write();
        drivers.retain(|d| d.name != name);
        Ok(())
    }
    
    /// Add a device and try to find a driver (like device_add in Linux)
    pub fn add_device(&self, device_id: ExtendedDeviceId, device: DeviceHandle) -> DeviceResult<()> {
        // Try to find matching driver
        let drivers = self.drivers.read();
        
        for driver in drivers.iter() {
            if driver.matches(&device_id) {
                // Found matching driver - probe it
                return self.probe_device(driver.clone(), device);
            }
        }
        
        // No driver found - add to pending
        let mut pending = self.pending_devices.write();
        pending.push((device_id, device));
        
        Ok(())
    }
    
    /// Probe a device with a driver
    fn probe_device(&self, driver: Arc<Driver>, device: DeviceHandle) -> DeviceResult<()> {
        let mut dev = device.write();
        
        match driver.probe_type {
            ProbeType::Synchronous | ProbeType::ForceSynchronous => {
                // Synchronous probe
                (driver.probe)(&mut *dev)?;
            }
            ProbeType::Asynchronous => {
                // TODO: Async probe (Phase II)
                (driver.probe)(&mut *dev)?;
            }
        }
        
        Ok(())
    }
    
    /// Try to match pending devices with a newly registered driver
    fn match_pending_devices(&self, driver: &Arc<Driver>) {
        let mut pending = self.pending_devices.write();
        let mut matched = Vec::new();
        
        for (i, (device_id, device)) in pending.iter().enumerate() {
            if driver.matches(device_id) {
                matched.push(i);
                let _ = self.probe_device(driver.clone(), device.clone());
            }
        }
        
        // Remove matched devices from pending (in reverse order)
        for i in matched.into_iter().rev() {
            pending.remove(i);
        }
    }
    
    /// Remove a device (like device_del in Linux)
    pub fn remove_device(&self, device: DeviceHandle) -> DeviceResult<()> {
        let drivers = self.drivers.read();
        let device_id = {
            let dev = device.read();
            dev.device_id()
        };
        
        // Find driver that handles this device
        for driver in drivers.iter() {
            let ext_id = ExtendedDeviceId::new(device_id.vendor, device_id.product);
            if driver.matches(&ext_id) {
                let mut dev = device.write();
                return (driver.remove)(&mut *dev);
            }
        }
        
        Err(DeviceError::NotSupported)
    }
    
    /// List all registered drivers
    pub fn list_drivers(&self) -> Vec<String> {
        let drivers = self.drivers.read();
        drivers.iter().map(|d| String::from(d.name)).collect()
    }
    
    /// Get driver count
    pub fn driver_count(&self) -> usize {
        self.drivers.read().len()
    }
    
    /// Get pending device count
    pub fn pending_count(&self) -> usize {
        self.pending_devices.read().len()
    }
}

/// Global driver manager instance
static DRIVER_MANAGER: DriverManager = DriverManager::new();

/// Get global driver manager
pub fn global_driver_manager() -> &'static DriverManager {
    &DRIVER_MANAGER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extended_device_id_match() {
        let id1 = ExtendedDeviceId::new(0x1234, 0x5678);
        let id2 = ExtendedDeviceId::new(0x1234, 0x5678);
        assert!(id1.matches(&id2));
        
        let id3 = ExtendedDeviceId::new(0x1234, ExtendedDeviceId::ANY);
        assert!(id3.matches(&id1));
    }

    #[test]
    fn test_extended_device_id_class_match() {
        let id1 = ExtendedDeviceId::with_class(0x1234, 0x5678, 0x030000, 0xFF0000);
        let id2 = ExtendedDeviceId::with_class(0x1234, 0x5678, 0x030100, 0xFF0000);
        assert!(id1.matches(&id2)); // Same class (0x03)
    }

    #[test]
    fn test_driver_matching() {
        let driver = Driver::new("test_driver", BusType::PCI)
            .with_ids(vec![
                ExtendedDeviceId::new(0x1234, 0x5678),
                ExtendedDeviceId::new(0xABCD, 0xEF01),
            ]);
        
        let id1 = ExtendedDeviceId::new(0x1234, 0x5678);
        assert!(driver.matches(&id1));
        
        let id2 = ExtendedDeviceId::new(0x9999, 0x8888);
        assert!(!driver.matches(&id2));
    }

    #[test]
    fn test_driver_manager() {
        let manager = DriverManager::new();
        
        let driver = Arc::new(Driver::new("test", BusType::I2C));
        assert!(manager.register_driver(driver).is_ok());
        assert_eq!(manager.driver_count(), 1);
    }
}
