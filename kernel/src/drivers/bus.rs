//! Bus Manager - Hardware bus abstraction and device discovery
//! 
//! Manages physical buses (I2C, SPI, UART, USB, PCI) and performs
//! device enumeration with vendor:product ID matching.

#[cfg(not(feature = "std"))]
extern crate alloc;

use crate::drivers::traits::DeviceId;
use crate::prelude::*;
use core::fmt;

/// Physical bus types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusType {
    I2C,
    SPI,
    UART,
    USB,
    PCI,
    Platform,  // Memory-mapped devices
}

impl fmt::Display for BusType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BusType::I2C => write!(f, "I2C"),
            BusType::SPI => write!(f, "SPI"),
            BusType::UART => write!(f, "UART"),
            BusType::USB => write!(f, "USB"),
            BusType::PCI => write!(f, "PCI"),
            BusType::Platform => write!(f, "Platform"),
        }
    }
}

/// Bus scanning result
#[derive(Debug, Clone)]
pub struct BusScanResult {
    pub bus_type: BusType,
    pub device_id: DeviceId,
    pub address: u64,
}

/// Bus manager errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusError {
    BusNotAvailable,
    ScanFailed,
    DeviceNotResponding,
    InvalidAddress,
}

pub type BusResult<T> = Result<T, BusError>;

/// Bus interface trait
pub trait Bus: Send + Sync {
    /// Get bus type
    fn bus_type(&self) -> BusType;
    
    /// Scan bus for devices
    fn scan(&mut self) -> BusResult<Vec<BusScanResult>>;
    
    /// Check if device is present at address
    fn probe(&self, address: u64) -> bool;
    
    /// Enable bus
    fn enable(&mut self) -> BusResult<()>;
    
    /// Disable bus
    fn disable(&mut self) -> BusResult<()>;
}

/// Global bus manager
pub struct BusManager {
    buses: Vec<&'static mut dyn Bus>,
}

impl BusManager {
    /// Create new bus manager
    pub const fn new() -> Self {
        Self {
            buses: Vec::new(),
        }
    }

    /// Register a bus
    pub fn register_bus(&mut self, bus: &'static mut dyn Bus) -> BusResult<()> {
        bus.enable()?;
        self.buses.push(bus);
        Ok(())
    }

    /// Scan all buses for devices
    pub fn scan_all(&mut self) -> Vec<BusScanResult> {
        let mut results = Vec::new();
        
        for bus in &mut self.buses {
            if let Ok(devices) = bus.scan() {
                results.extend(devices);
            }
        }
        
        results
    }

    /// Find device on specific bus type
    pub fn find_device(&mut self, bus_type: BusType, device_id: DeviceId) -> Option<BusScanResult> {
        for bus in &mut self.buses {
            if bus.bus_type() == bus_type {
                if let Ok(devices) = bus.scan() {
                    for result in devices {
                        if result.device_id == device_id {
                            return Some(result);
                        }
                    }
                }
            }
        }
        None
    }

    /// Get number of registered buses
    pub fn bus_count(&self) -> usize {
        self.buses.len()
    }
}

/// Global bus manager instance
struct BusManagerCell(core::cell::UnsafeCell<BusManager>);
unsafe impl Sync for BusManagerCell {}
static BUS_MANAGER: BusManagerCell = BusManagerCell(core::cell::UnsafeCell::new(BusManager::new()));

/// Get global bus manager
/// # Safety: caller must ensure exclusive access
pub unsafe fn global_bus_manager() -> &'static mut BusManager {
    &mut *BUS_MANAGER.0.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBus {
        bus_type: BusType,
        devices: Vec<BusScanResult>,
    }

    impl Bus for MockBus {
        fn bus_type(&self) -> BusType {
            self.bus_type
        }

        fn scan(&mut self) -> BusResult<Vec<BusScanResult>> {
            Ok(self.devices.clone())
        }

        fn probe(&self, address: u64) -> bool {
            self.devices.iter().any(|d| d.address == address)
        }

        fn enable(&mut self) -> BusResult<()> {
            Ok(())
        }

        fn disable(&mut self) -> BusResult<()> {
            Ok(())
        }
    }

    #[test]
    fn test_bus_type_display() {
        assert_eq!(format!("{}", BusType::I2C), "I2C");
        assert_eq!(format!("{}", BusType::USB), "USB");
    }

    #[test]
    fn test_bus_manager_scan() {
        let mut manager = BusManager::new();
        
        let device_id = DeviceId::new(0x1000, 0x2000, 0);
        let mock_bus = Box::leak(Box::new(MockBus {
            bus_type: BusType::I2C,
            devices: vec![BusScanResult {
                bus_type: BusType::I2C,
                device_id,
                address: 0x50,
            }],
        }));
        
        manager.register_bus(mock_bus).unwrap();
        let results = manager.scan_all();
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].device_id, device_id);
    }
}
