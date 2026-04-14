//! Mock device driver for testing Ghost Bus architecture
//! 
//! Simulates any device type with configurable behavior and failure modes.

use crate::drivers::traits::*;
use core::sync::atomic::{AtomicU32, Ordering};

/// Mock device configuration
#[derive(Debug, Clone, Default)]
pub struct MockConfig {
    pub device_id: DeviceId,
    pub name: &'static str,
    pub capabilities: DeviceCapabilities,
    pub fail_init: bool,
    pub fail_shutdown: bool,
}

/// Mock device implementation
pub struct MockDevice {
    config: MockConfig,
    initialized: bool,
    power_state: PowerState,
    call_count: AtomicU32,
}

impl MockDevice {
    pub fn new(config: MockConfig) -> Self {
        Self {
            config,
            initialized: false,
            power_state: PowerState::Off,
            call_count: AtomicU32::new(0),
        }
    }

    pub fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::Relaxed)
    }
}

impl BasicDevice for MockDevice {
    fn init(&mut self) -> DeviceResult<()> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        
        if self.config.fail_init {
            return Err(DeviceError::HardwareFailure);
        }
        
        self.initialized = true;
        self.power_state = PowerState::Active;
        Ok(())
    }

    fn shutdown(&mut self) -> DeviceResult<()> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        
        if self.config.fail_shutdown {
            return Err(DeviceError::HardwareFailure);
        }
        
        self.initialized = false;
        self.power_state = PowerState::Off;
        Ok(())
    }

    fn power_save(&mut self, state: PowerState) -> DeviceResult<()> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        
        if !self.initialized {
            return Err(DeviceError::NotInitialized);
        }
        
        self.power_state = state;
        Ok(())
    }

    fn device_id(&self) -> DeviceId {
        self.config.device_id
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.config.capabilities
    }

    fn name(&self) -> &str {
        self.config.name
    }
}

/// Mock sensor data
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MockSensorData {
    pub value: f32,
    pub timestamp: u64,
}

/// Mock streaming device
pub struct MockStreamDevice {
    base: MockDevice,
    streaming: bool,
    sample_rate: u32,
    sample_counter: AtomicU32,
}

impl MockStreamDevice {
    pub fn new(config: MockConfig, sample_rate: u32) -> Self {
        Self {
            base: MockDevice::new(config),
            streaming: false,
            sample_rate,
            sample_counter: AtomicU32::new(0),
        }
    }
}

impl BasicDevice for MockStreamDevice {
    fn init(&mut self) -> DeviceResult<()> {
        self.base.init()
    }

    fn shutdown(&mut self) -> DeviceResult<()> {
        self.streaming = false;
        self.base.shutdown()
    }

    fn power_save(&mut self, state: PowerState) -> DeviceResult<()> {
        self.base.power_save(state)
    }

    fn device_id(&self) -> DeviceId {
        self.base.device_id()
    }

    fn capabilities(&self) -> DeviceCapabilities {
        self.base.capabilities()
    }

    fn name(&self) -> &str {
        self.base.name()
    }
}

impl Streamable for MockStreamDevice {
    type Data = MockSensorData;

    fn poll(&mut self) -> DeviceResult<Option<Self::Data>> {
        if !self.streaming {
            return Ok(None);
        }

        let counter = self.sample_counter.fetch_add(1, Ordering::Relaxed);
        Ok(Some(MockSensorData {
            value: (counter as f32) * 0.1,
            timestamp: counter as u64,
        }))
    }

    fn start_stream(&mut self) -> DeviceResult<()> {
        if !self.base.initialized {
            return Err(DeviceError::NotInitialized);
        }
        self.streaming = true;
        Ok(())
    }

    fn stop_stream(&mut self) -> DeviceResult<()> {
        self.streaming = false;
        Ok(())
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_device_lifecycle() {
        let config = MockConfig::default();
        let mut device = MockDevice::new(config);
        
        assert!(device.init().is_ok());
        assert_eq!(device.power_state, PowerState::Active);
        assert!(device.shutdown().is_ok());
        assert_eq!(device.power_state, PowerState::Off);
    }

    #[test]
    fn test_mock_device_failure() {
        let mut config = MockConfig::default();
        config.fail_init = true;
        let mut device = MockDevice::new(config);
        
        assert_eq!(device.init(), Err(DeviceError::HardwareFailure));
    }

    #[test]
    fn test_mock_stream_device() {
        let config = MockConfig::default();
        let mut device = MockStreamDevice::new(config, 1000);
        
        device.init().unwrap();
        device.start_stream().unwrap();
        
        let data1 = device.poll().unwrap().unwrap();
        let data2 = device.poll().unwrap().unwrap();
        
        assert_eq!(data1.value, 0.0);
        assert_eq!(data2.value, 0.1);
        assert_eq!(device.sample_rate(), 1000);
    }

    #[test]
    fn test_call_counting() {
        let config = MockConfig::default();
        let mut device = MockDevice::new(config);
        
        assert_eq!(device.call_count(), 0);
        device.init().unwrap();
        assert_eq!(device.call_count(), 1);
        device.power_save(PowerState::Suspend).unwrap();
        assert_eq!(device.call_count(), 2);
    }
}
