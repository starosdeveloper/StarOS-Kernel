//! Device Tree Integration - Create devices from DTB
//! 
//! Integrates existing devicetree parser with driver manager

#[cfg(not(feature = "std"))]
extern crate alloc;

use crate::drivers::traits::{DeviceId, DeviceCapabilities, BasicDevice, DeviceResult, DeviceError, PowerState};
use crate::prelude::*;
use crate::drivers::driver_manager::{ExtendedDeviceId, global_driver_manager};
use crate::drivers::registry::{DeviceHandle, global_registry};
use crate::drivers::bus::BusType;
use crate::devicetree::parser::FdtParser;
use spin::RwLock;

/// Device Tree device (platform device)
pub struct DtDevice {
    id: DeviceId,
    name: String,
    compatible: String,
    reg: Vec<(u64, u64)>, // (address, size) pairs
    interrupts: Vec<u32>,
    initialized: bool,
}

impl DtDevice {
    pub fn from_dt_node(parser: &FdtParser, node_offset: usize) -> DeviceResult<Self> {
        // Simplified version - just create a basic device
        // Full DT parsing will be in Phase II
        
        let compatible = String::from("platform-device");
        let vendor = 0x0000u16;
        let device = (node_offset as u16) & 0xFFFF;
        
        Ok(Self {
            id: DeviceId::new(vendor, device, 0),
            name: compatible.clone(),
            compatible,
            reg: Vec::new(),
            interrupts: Vec::new(),
            initialized: false,
        })
    }
    
    pub fn compatible(&self) -> &str {
        &self.compatible
    }
    
    pub fn mmio_base(&self) -> Option<u64> {
        self.reg.first().map(|(addr, _)| *addr)
    }
    
    pub fn mmio_size(&self) -> Option<u64> {
        self.reg.first().map(|(_, size)| *size)
    }
}

impl BasicDevice for DtDevice {
    fn init(&mut self) -> DeviceResult<()> {
        self.initialized = true;
        Ok(())
    }
    
    fn shutdown(&mut self) -> DeviceResult<()> {
        self.initialized = false;
        Ok(())
    }
    
    fn power_save(&mut self, _state: PowerState) -> DeviceResult<()> {
        Ok(())
    }
    
    fn device_id(&self) -> DeviceId {
        self.id
    }
    
    fn capabilities(&self) -> DeviceCapabilities {
        DeviceCapabilities {
            can_stream: false,
            can_block_io: !self.reg.is_empty(),
            supports_dma: false,
            hot_pluggable: false,
        }
    }
    
    fn name(&self) -> &str {
        &self.name
    }
}

/// Device Tree enumerator
pub struct DtEnumerator;

impl DtEnumerator {
    /// Enumerate all devices from Device Tree
    pub fn enumerate(dtb_addr: usize) -> DeviceResult<Vec<DeviceHandle>> {
        let parser = FdtParser::new(dtb_addr).map_err(|_| DeviceError::HardwareFailure)?;
        let mut devices = Vec::new();
        
        // Walk device tree and create devices
        Self::walk_tree(&parser, 0, &mut devices)?;
        
        Ok(devices)
    }
    
    fn walk_tree(parser: &FdtParser, offset: usize, devices: &mut Vec<DeviceHandle>) -> DeviceResult<()> {
        // Try to create device from this node
        if let Ok(dt_device) = DtDevice::from_dt_node(parser, offset) {
            let handle = Arc::new(RwLock::new(dt_device));
            devices.push(handle);
        }
        
        // TODO: Walk child nodes (requires parser enhancement)
        
        Ok(())
    }
    
    /// Register all DT devices with driver manager
    pub fn register_all(dtb_addr: usize) -> DeviceResult<usize> {
        let devices = Self::enumerate(dtb_addr)?;
        let count = devices.len();
        
        for device in devices {
            let (id, ext_id) = {
                let dev = device.read();
                let id = dev.device_id();
                let ext_id = ExtendedDeviceId::new(id.vendor, id.product);
                (id, ext_id)
            };
            
            // Register in global registry
            global_registry().register(id, device.clone())
                .map_err(|_| DeviceError::HardwareFailure)?;
            
            // Try to match with driver
            let _ = global_driver_manager().add_device(ext_id, device);
        }
        
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dt_device_creation() {
        let id = DeviceId::new(0x1234, 0x5678, 0);
        let device = DtDevice {
            id,
            name: String::from("test"),
            compatible: String::from("vendor,device"),
            reg: vec![(0x1000, 0x100)],
            interrupts: vec![42],
            initialized: false,
        };
        
        assert_eq!(device.mmio_base(), Some(0x1000));
        assert_eq!(device.mmio_size(), Some(0x100));
    }
}
