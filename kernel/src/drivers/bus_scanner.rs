//! Bus Scanning & Enumeration - Linux-inspired automatic device discovery
//! 
//! Implements bus_probe_device() and bus_rescan_devices() from Linux

#[cfg(not(feature = "std"))]
extern crate alloc;

use crate::drivers::traits::{DeviceId, BasicDevice, DeviceResult, DeviceError};
use crate::prelude::*;
use crate::drivers::bus::{Bus, BusType, BusScanResult};
use crate::drivers::registry::{DeviceHandle, global_registry};
use crate::drivers::driver_manager::{ExtendedDeviceId, global_driver_manager};
use spin::RwLock;

/// Bus scanner - finds devices on buses
pub struct BusScanner {
    scan_count: core::sync::atomic::AtomicU64,
}

impl BusScanner {
    pub const fn new() -> Self {
        Self {
            scan_count: core::sync::atomic::AtomicU64::new(0),
        }
    }
    
    /// Scan a single bus for devices (like bus_probe_device in Linux)
    pub fn scan_bus(&self, bus: &mut dyn Bus) -> DeviceResult<Vec<BusScanResult>> {
        bus.scan().map_err(|_| DeviceError::HardwareFailure)
    }
    
    /// Probe a discovered device (auto-match with driver)
    pub fn probe_device(&self, scan_result: &BusScanResult, device: DeviceHandle) -> DeviceResult<()> {
        let ext_id = ExtendedDeviceId::new(
            scan_result.device_id.vendor,
            scan_result.device_id.product,
        );
        
        // Register in global registry
        global_registry().register(scan_result.device_id, device.clone())
            .map_err(|_| DeviceError::HardwareFailure)?;
        
        // Try to find and probe driver
        global_driver_manager().add_device(ext_id, device)?;
        
        Ok(())
    }
    
    /// Rescan all buses (like bus_rescan_devices in Linux)
    pub fn rescan_all(&self, buses: &mut [&mut dyn Bus]) -> DeviceResult<usize> {
        let mut total_found = 0;
        
        for bus in buses {
            match self.scan_bus(*bus) {
                Ok(devices) => {
                    total_found += devices.len();
                    // Devices will be probed by driver manager
                }
                Err(_) => continue,
            }
        }
        
        self.scan_count.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        Ok(total_found)
    }
    
    /// Get scan count
    pub fn scan_count(&self) -> u64 {
        self.scan_count.load(core::sync::atomic::Ordering::Relaxed)
    }
}

/// Global bus scanner
static BUS_SCANNER: BusScanner = BusScanner::new();

pub fn global_bus_scanner() -> &'static BusScanner {
    &BUS_SCANNER
}

/// Hot-plug event handler
pub struct HotplugHandler {
    enabled: core::sync::atomic::AtomicBool,
}

impl HotplugHandler {
    pub const fn new() -> Self {
        Self {
            enabled: core::sync::atomic::AtomicBool::new(false),
        }
    }
    
    /// Enable hot-plug detection
    pub fn enable(&self) {
        self.enabled.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    
    /// Disable hot-plug detection
    pub fn disable(&self) {
        self.enabled.store(false, core::sync::atomic::Ordering::Relaxed);
    }
    
    /// Handle device insertion
    pub fn device_inserted(&self, scan_result: BusScanResult, device: DeviceHandle) -> DeviceResult<()> {
        if !self.enabled.load(core::sync::atomic::Ordering::Relaxed) {
            return Err(DeviceError::NotSupported);
        }
        
        global_bus_scanner().probe_device(&scan_result, device)
    }
    
    /// Handle device removal
    pub fn device_removed(&self, device: DeviceHandle) -> DeviceResult<()> {
        if !self.enabled.load(core::sync::atomic::Ordering::Relaxed) {
            return Err(DeviceError::NotSupported);
        }
        
        global_driver_manager().remove_device(device)
    }
}

static HOTPLUG_HANDLER: HotplugHandler = HotplugHandler::new();

pub fn global_hotplug_handler() -> &'static HotplugHandler {
    &HOTPLUG_HANDLER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bus_scanner() {
        let scanner = BusScanner::new();
        assert_eq!(scanner.scan_count(), 0);
    }

    #[test]
    fn test_hotplug_handler() {
        let handler = HotplugHandler::new();
        handler.enable();
        assert!(handler.enabled.load(core::sync::atomic::Ordering::Relaxed));
    }
}
