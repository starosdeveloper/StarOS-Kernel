//! Device drivers - Ghost Bus Architecture

// Linux driver model (base infrastructure)
#[cfg(not(test))]
pub mod base;

// Ghost Bus core modules
pub mod traits;
pub mod registry;
pub mod bus;
pub mod mock;
#[cfg(not(test))]
pub mod driver_manager;
#[cfg(not(test))]
pub mod linux_compat;
#[cfg(not(test))]
pub mod bus_scanner;
#[cfg(not(test))]
pub mod dt_integration;
pub mod resource;

// Device Tree (Open Firmware)
#[cfg(not(test))]
pub mod of;

// I2C Subsystem
#[cfg(not(test))]
pub mod i2c;

// SPI Subsystem
#[cfg(not(test))]
pub mod spi;

// Display subsystem
pub mod display;

// Clock Framework
pub mod clk;

// DMA Engine
#[cfg(not(test))]
pub mod dma;

// Network drivers (WiFi, Ethernet)
pub mod net;

// Bluetooth drivers
pub mod bluetooth;

// WWAN / Modem drivers (QMI, AT commands)
pub mod wwan;

// GPU / Display drivers (DRM/KMS, MIPI DSI)
pub mod gpu;

// Media drivers (V4L2, MIPI CSI, ISP)
pub mod media;

// Legacy drivers (to be migrated)
pub mod uart;
pub mod devicetree;
pub mod gpio;

// Samsung Exynos/One UI device support
#[cfg(not(test))]
pub mod samsung;
pub mod xiaomi;

// Re-exports
pub use traits::{BasicDevice, Streamable, BlockStorage, InterruptDevice, DeviceId, DeviceCapabilities, PowerState};
pub use registry::{DeviceRegistry, DeviceHandle, RegistryError, global_registry};
pub use bus::{BusManager, BusType, Bus, BusScanResult, BusError, global_bus_manager};
pub use mock::{MockDevice, MockStreamDevice, MockConfig, MockSensorData};
#[cfg(not(test))]
pub use driver_manager::{Driver, DriverManager, ExtendedDeviceId, ProbeType, global_driver_manager};
#[cfg(not(test))]
pub use bus_scanner::{BusScanner, HotplugHandler, global_bus_scanner, global_hotplug_handler};
#[cfg(not(test))]
pub use dt_integration::{DtDevice, DtEnumerator};
pub use resource::{Resource, ResourceConstraint, ResourceDesc, ResourceError, Result as ResourceResult};

// Legacy exports
pub use uart::Uart;
pub use devicetree::{DeviceTree, DeviceNode, DeviceDiscovery};
pub use gpio::{GpioPin, GpioController, GpioDirection, GpioValue};
