//! Core device traits for StarOS Ghost Bus architecture
//! 
//! This module defines the fundamental interfaces that ALL hardware devices
//! must implement. Zero hardware-specific code allowed in kernel core.

use core::fmt;

/// Unique device identifier (vendor:product:instance)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DeviceId {
    pub vendor: u16,
    pub product: u16,
    pub instance: u8,
}

impl DeviceId {
    pub const fn new(vendor: u16, product: u16, instance: u8) -> Self {
        Self { vendor, product, instance }
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}:{:04x}:{:02x}", self.vendor, self.product, self.instance)
    }
}

/// Device capability flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceCapabilities {
    pub can_stream: bool,
    pub can_block_io: bool,
    pub supports_dma: bool,
    pub hot_pluggable: bool,
}

/// Device power state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    Off,
    Suspend,
    Active,
    HighPerformance,
}

/// Device error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    NotInitialized,
    HardwareFailure,
    Timeout,
    InvalidOperation,
    BufferOverflow,
    NotSupported,
}

pub type DeviceResult<T> = Result<T, DeviceError>;

/// Basic device lifecycle - ALL devices must implement this
pub trait BasicDevice: Send + Sync {
    /// Initialize device hardware
    fn init(&mut self) -> DeviceResult<()>;
    
    /// Shutdown device gracefully
    fn shutdown(&mut self) -> DeviceResult<()>;
    
    /// Enter power saving mode
    fn power_save(&mut self, state: PowerState) -> DeviceResult<()>;
    
    /// Get device unique identifier
    fn device_id(&self) -> DeviceId;
    
    /// Get device capabilities
    fn capabilities(&self) -> DeviceCapabilities;
    
    /// Device name for logging
    fn name(&self) -> &str;
}

/// Streaming device interface (sensors, audio, video)
pub trait Streamable: BasicDevice {
    /// Data type this device streams
    type Data: Send;
    
    /// Poll for single data sample (non-blocking)
    fn poll(&mut self) -> DeviceResult<Option<Self::Data>>;
    
    /// Start continuous streaming
    fn start_stream(&mut self) -> DeviceResult<()>;
    
    /// Stop streaming
    fn stop_stream(&mut self) -> DeviceResult<()>;
    
    /// Get stream sample rate (Hz)
    fn sample_rate(&self) -> u32;
}

/// Block storage device interface (flash, SD card, etc)
pub trait BlockStorage: BasicDevice {
    /// Read blocks from device
    fn read_blocks(&mut self, start: u64, count: u32, buf: &mut [u8]) -> DeviceResult<usize>;
    
    /// Write blocks to device
    fn write_blocks(&mut self, start: u64, buf: &[u8]) -> DeviceResult<usize>;
    
    /// Flush write cache
    fn flush(&mut self) -> DeviceResult<()>;
    
    /// Get block size in bytes
    fn block_size(&self) -> u32;
    
    /// Get total capacity in blocks
    fn capacity(&self) -> u64;
}

/// Interrupt-capable device
pub trait InterruptDevice: BasicDevice {
    /// Enable interrupts
    fn enable_interrupts(&mut self) -> DeviceResult<()>;
    
    /// Disable interrupts
    fn disable_interrupts(&mut self) -> DeviceResult<()>;
    
    /// Handle interrupt (called from ISR context - must be fast!)
    fn handle_interrupt(&mut self) -> DeviceResult<()>;
    
    /// Get interrupt number
    fn interrupt_number(&self) -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id_format() {
        let id = DeviceId::new(0x1234, 0x5678, 0);
        assert_eq!(format!("{}", id), "1234:5678:00");
    }

    #[test]
    fn test_device_id_equality() {
        let id1 = DeviceId::new(0x1000, 0x2000, 0);
        let id2 = DeviceId::new(0x1000, 0x2000, 0);
        let id3 = DeviceId::new(0x1000, 0x2000, 1);
        
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}
