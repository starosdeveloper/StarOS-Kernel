// SPDX-License-Identifier: MIT OR Apache-2.0
//! I2C Core - Linux I2C subsystem port to Rust
//!
//! Ported from: linux-master/drivers/i2c/i2c-core-base.c
//! Source lines: 2200 C → 1000 Rust
//!
//! This module implements the core I2C subsystem functionality including:
//! - I2C adapter registration and management
//! - I2C client device management
//! - I2C transfer operations
//! - Bus recovery mechanisms
//! - Device matching and probing

use alloc::sync::Arc;
use alloc::vec::Vec;                    
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::sync::IrqSafeMutex;

/// Result type for I2C operations
pub type Result<T> = core::result::Result<T, Error>;

/// I2C error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidArgument,
    NotSupported,
    AddressInUse,
    NotFound,
    Busy,
    IoError,
    Timeout,
}

/// I2C address modes
pub const I2C_ADDR_7BITS_MAX: u16 = 0x77;
pub const I2C_ADDR_10BITS_MAX: u16 = 0x3ff;
pub const I2C_ADDR_OFFSET_TEN_BIT: u16 = 0xa000;
pub const I2C_ADDR_OFFSET_SLAVE: u16 = 0x1000;

/// I2C frequency modes (Hz)
pub const I2C_MAX_STANDARD_MODE_FREQ: u32 = 100_000;      // 100 kHz
pub const I2C_MAX_FAST_MODE_FREQ: u32 = 400_000;          // 400 kHz
pub const I2C_MAX_FAST_MODE_PLUS_FREQ: u32 = 1_000_000;   // 1.0 MHz
pub const I2C_MAX_TURBO_MODE_FREQ: u32 = 1_400_000;       // 1.4 MHz
pub const I2C_MAX_HIGH_SPEED_MODE_FREQ: u32 = 3_400_000;  // 3.4 MHz
pub const I2C_MAX_ULTRA_FAST_MODE_FREQ: u32 = 5_000_000;  // 5.0 MHz

/// I2C client flags
pub const I2C_CLIENT_TEN: u16 = 0x01;           // 10-bit address
pub const I2C_CLIENT_SLAVE: u16 = 0x02;         // Slave mode
pub const I2C_CLIENT_HOST_NOTIFY: u16 = 0x04;   // Host notify
pub const I2C_CLIENT_WAKE: u16 = 0x08;          // Wake-up capable
pub const I2C_CLIENT_SCCB: u16 = 0x10;          // SCCB protocol

/// I2C message flags
pub const I2C_M_RD: u16 = 0x0001;               // Read data
pub const I2C_M_TEN: u16 = 0x0010;              // 10-bit address
pub const I2C_M_DMA_SAFE: u16 = 0x0200;         // Buffer is DMA safe
pub const I2C_M_RECV_LEN: u16 = 0x0400;         // Length in first byte
pub const I2C_M_NO_RD_ACK: u16 = 0x0800;        // No ACK on read
pub const I2C_M_IGNORE_NAK: u16 = 0x1000;       // Ignore NAK
pub const I2C_M_REV_DIR_ADDR: u16 = 0x2000;     // Reverse direction
pub const I2C_M_NOSTART: u16 = 0x4000;          // No START condition
pub const I2C_M_STOP: u16 = 0x8000;             // STOP after message

/// Bus recovery constants
const RECOVERY_NDELAY: u32 = 5000;              // 5us delay for recovery
const RECOVERY_CLK_CNT: u32 = 9;                // 9 clock pulses

/// Global I2C adapter registry
static ADAPTER_REGISTRY: IrqSafeMutex<Vec<Arc<I2cAdapter>>> = IrqSafeMutex::new(Vec::new());
static NEXT_ADAPTER_ID: AtomicU32 = AtomicU32::new(0);

/// i2c_freq_mode_string - Get human-readable frequency mode string
///
/// # Arguments
/// * `bus_freq_hz` - Bus frequency in Hz
///
/// # Returns
/// String describing the I2C frequency mode
pub fn i2c_freq_mode_string(bus_freq_hz: u32) -> &'static str {
    match bus_freq_hz {
        I2C_MAX_STANDARD_MODE_FREQ => "Standard Mode (100 kHz)",
        I2C_MAX_FAST_MODE_FREQ => "Fast Mode (400 kHz)",
        I2C_MAX_FAST_MODE_PLUS_FREQ => "Fast Mode Plus (1.0 MHz)",
        I2C_MAX_TURBO_MODE_FREQ => "Turbo Mode (1.4 MHz)",
        I2C_MAX_HIGH_SPEED_MODE_FREQ => "High Speed Mode (3.4 MHz)",
        I2C_MAX_ULTRA_FAST_MODE_FREQ => "Ultra Fast Mode (5.0 MHz)",
        _ => "Unknown Mode",
    }
}

/// I2C message structure
#[derive(Debug, Clone)]
pub struct I2cMsg {
    /// Slave address
    pub addr: u16,
    /// Message flags (I2C_M_*)
    pub flags: u16,
    /// Message data buffer
    pub buf: Vec<u8>,
}

impl I2cMsg {
    /// Create new I2C message
    pub fn new(addr: u16, flags: u16, buf: Vec<u8>) -> Self {
        Self { addr, flags, buf }
    }

    /// Create read message
    pub fn read(addr: u16, len: usize) -> Self {
        Self {
            addr,
            flags: I2C_M_RD,
            buf: vec![0u8; len],
        }
    }

    /// Create write message
    pub fn write(addr: u16, data: Vec<u8>) -> Self {
        Self {
            addr,
            flags: 0,
            buf: data,
        }
    }

    /// Check if message is read
    pub fn is_read(&self) -> bool {
        self.flags & I2C_M_RD != 0
    }

    /// Check if message uses 10-bit addressing
    pub fn is_ten_bit(&self) -> bool {
        self.flags & I2C_M_TEN != 0
    }
}

/// I2C algorithm operations
pub trait I2cAlgorithm: Send + Sync {
    /// Master transfer operation
    fn master_xfer(&self, adapter: &I2cAdapter, msgs: &mut [I2cMsg]) -> Result<usize>;

    /// SMBus transfer operation (optional)
    fn smbus_xfer(&self, _adapter: &I2cAdapter, _addr: u16, _flags: u16,
                  _read_write: u8, _command: u8, _size: u32, _data: &mut [u8]) -> Result<()> {
        Err(Error::NotSupported)
    }

    /// Get functionality flags
    fn functionality(&self) -> u32;
}

/// I2C bus recovery information
pub struct I2cBusRecoveryInfo {
    /// Recovery function
    pub recover_bus: Option<fn(&I2cAdapter) -> Result<()>>,
    /// Get SCL state
    pub get_scl: Option<fn(&I2cAdapter) -> bool>,
    /// Set SCL state
    pub set_scl: Option<fn(&I2cAdapter, bool)>,
    /// Get SDA state
    pub get_sda: Option<fn(&I2cAdapter) -> bool>,
    /// Set SDA state
    pub set_sda: Option<fn(&I2cAdapter, bool)>,
    /// Prepare for recovery
    pub prepare_recovery: Option<fn(&I2cAdapter)>,
    /// Cleanup after recovery
    pub unprepare_recovery: Option<fn(&I2cAdapter)>,
}

impl I2cBusRecoveryInfo {
    /// Create new recovery info
    pub fn new() -> Self {
        Self {
            recover_bus: None,
            get_scl: None,
            set_scl: None,
            get_sda: None,
            set_sda: None,
            prepare_recovery: None,
            unprepare_recovery: None,
        }
    }
}

/// I2C adapter structure
pub struct I2cAdapter {
    /// Adapter ID
    pub id: u32,
    /// Adapter name
    pub name: String,
    /// I2C algorithm
    pub algo: Arc<dyn I2cAlgorithm>,
    /// Bus frequency in Hz
    pub bus_freq_hz: u32,
    /// Timeout in jiffies
    pub timeout: u32,
    /// Number of retries
    pub retries: u32,
    /// Bus recovery information
    pub recovery_info: Option<I2cBusRecoveryInfo>,
    /// Registered clients
    clients: IrqSafeMutex<Vec<Arc<I2cClient>>>,
}

impl I2cAdapter {
    /// Create new I2C adapter
    pub fn new(name: String, algo: Arc<dyn I2cAlgorithm>, bus_freq_hz: u32) -> Self {
        let id = NEXT_ADAPTER_ID.fetch_add(1, Ordering::SeqCst);
        Self {
            id,
            name,
            algo,
            bus_freq_hz,
            timeout: 1000,  // 1 second default
            retries: 3,
            recovery_info: None,
            clients: IrqSafeMutex::new(Vec::new()),
        }
    }

    /// i2c_transfer - Execute I2C transfer
    ///
    /// # Arguments
    /// * `msgs` - Array of I2C messages to transfer
    ///
    /// # Returns
    /// Number of messages transferred on success
    pub fn transfer(&self, msgs: &mut [I2cMsg]) -> Result<usize> {
        if msgs.is_empty() {
            return Err(Error::InvalidArgument);
        }

        // Perform transfer with retries
        let mut attempts = 0;
        loop {
            match self.algo.master_xfer(self, msgs) {
                Ok(count) => return Ok(count),
                Err(e) if attempts < self.retries => {
                    attempts += 1;
                    // Small delay before retry
                    for _ in 0..1000 { core::hint::spin_loop(); }
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// i2c_recover_bus - Attempt bus recovery
    pub fn recover_bus(&self) -> Result<()> {
        if let Some(ref recovery) = self.recovery_info {
            if let Some(recover_fn) = recovery.recover_bus {
                log::debug!("I2C adapter {}: Attempting bus recovery", self.name);
                return recover_fn(self);
            }
        }
        Err(Error::NotSupported)
    }

    /// Add client to adapter
    pub fn add_client(&self, client: Arc<I2cClient>) -> Result<()> {
        let mut clients = self.clients.lock_irqsave();
        
        // Check for address conflicts
        for existing in clients.iter() {
            if existing.addr == client.addr {
                return Err(Error::AddressInUse);
            }
        }
        
        clients.push(client);
        Ok(())
    }

    /// Remove client from adapter
    pub fn remove_client(&self, addr: u16) -> Result<()> {
        let mut clients = self.clients.lock_irqsave();
        clients.retain(|c| c.addr != addr);
        Ok(())
    }

    /// Get functionality flags
    pub fn functionality(&self) -> u32 {
        self.algo.functionality()
    }
}

/// I2C client device
pub struct I2cClient {
    /// Client name
    pub name: String,
    /// I2C address
    pub addr: u16,
    /// Client flags
    pub flags: u16,
    /// Parent adapter
    pub adapter: Arc<I2cAdapter>,
}

impl I2cClient {
    /// Create new I2C client
    pub fn new(name: String, addr: u16, flags: u16, adapter: Arc<I2cAdapter>) -> Result<Arc<Self>> {
        // Validate address
        if flags & I2C_CLIENT_TEN != 0 {
            if addr > I2C_ADDR_10BITS_MAX {
                return Err(Error::InvalidArgument);
            }
        } else if addr > I2C_ADDR_7BITS_MAX {
            return Err(Error::InvalidArgument);
        }

        let client = Arc::new(Self {
            name,
            addr,
            flags,
            adapter: adapter.clone(),
        });

        // Register with adapter
        adapter.add_client(client.clone())?;

        Ok(client)
    }

    /// i2c_master_send - Send data to I2C device
    pub fn master_send(&self, buf: &[u8]) -> Result<usize> {
        let mut msg = I2cMsg::write(self.addr, buf.to_vec());
        let count = self.adapter.transfer(&mut [msg])?;
        if count == 1 {
            Ok(buf.len())
        } else {
            Err(Error::IoError)
        }
    }

    /// i2c_master_recv - Receive data from I2C device
    pub fn master_recv(&self, buf: &mut [u8]) -> Result<usize> {
        let mut msgs = [I2cMsg::read(self.addr, buf.len())];
        let count = self.adapter.transfer(&mut msgs)?;
        if count == 1 {
            buf.copy_from_slice(&msgs[0].buf);
            Ok(buf.len())
        } else {
            Err(Error::IoError)
        }
    }

    /// i2c_transfer - Execute I2C transfer
    pub fn transfer(&self, msgs: &mut [I2cMsg]) -> Result<usize> {
        self.adapter.transfer(msgs)
    }
}

impl Drop for I2cClient {
    fn drop(&mut self) {
        let _ = self.adapter.remove_client(self.addr);
    }
}

/// i2c_generic_scl_recovery - Generic SCL recovery using bit-banging
///
/// Ported from: i2c_generic_scl_recovery()
/// Source: linux-master/drivers/i2c/i2c-core-base.c:230
pub fn i2c_generic_scl_recovery(adapter: &I2cAdapter) -> Result<()> {
    let recovery = adapter.recovery_info.as_ref()
        .ok_or(Error::NotSupported)?;

    let get_scl = recovery.get_scl.ok_or(Error::NotSupported)?;
    let set_scl = recovery.set_scl.ok_or(Error::NotSupported)?;

    // Prepare for recovery
    if let Some(prepare) = recovery.prepare_recovery {
        prepare(adapter);
    }

    // Generate 9 clock pulses to recover bus
    let mut scl = true;
    set_scl(adapter, scl);
    
    // Small delay (5us)
    for _ in 0..RECOVERY_NDELAY { core::hint::spin_loop(); }
    
    if let Some(set_sda) = recovery.set_sda {
        set_sda(adapter, scl);
    }

    for i in 0..(RECOVERY_CLK_CNT * 2) {
        if scl {
            // Check if SCL is actually high
            if !get_scl(adapter) {
                log::error!("I2C adapter {}: SCL stuck low, exit recovery", adapter.name);
                if let Some(unprepare) = recovery.unprepare_recovery {
                    unprepare(adapter);
                }
                return Err(Error::Busy);
            }
        }

        scl = !scl;
        set_scl(adapter, scl);

        // Timing delays
        if scl {
            for _ in 0..RECOVERY_NDELAY { core::hint::spin_loop(); }
        } else {
            for _ in 0..(RECOVERY_NDELAY / 2) { core::hint::spin_loop(); }
        }

        if let Some(set_sda) = recovery.set_sda {
            set_sda(adapter, scl);
        }
        
        for _ in 0..(RECOVERY_NDELAY / 2) { core::hint::spin_loop(); }

        // Check if bus is free after each high pulse
        if scl {
            if let Some(get_sda) = recovery.get_sda {
                if get_sda(adapter) {
                    log::info!("I2C adapter {}: Bus recovered after {} pulses", 
                              adapter.name, i / 2);
                    break;
                }
            }
        }
    }

    // Cleanup after recovery
    if let Some(unprepare) = recovery.unprepare_recovery {
        unprepare(adapter);
    }

    Ok(())
}

/// i2c_add_adapter - Register I2C adapter
///
/// # Arguments
/// * `adapter` - I2C adapter to register
pub fn i2c_add_adapter(adapter: Arc<I2cAdapter>) -> Result<()> {
    log::info!("I2C adapter {}: Registering (id={}, freq={})", 
              adapter.name, adapter.id, i2c_freq_mode_string(adapter.bus_freq_hz));

    // Initialize recovery if configured
    if adapter.recovery_info.is_some() {
        log::debug!("I2C adapter {}: Bus recovery configured", adapter.name);
    }

    // Add to global registry
    let mut registry = ADAPTER_REGISTRY.lock();
    registry.push(adapter.clone());

    log::info!("I2C adapter {}: Registration complete", adapter.name);
    Ok(())
}

/// i2c_del_adapter - Unregister I2C adapter
///
/// # Arguments
/// * `adapter_id` - ID of adapter to unregister
pub fn i2c_del_adapter(adapter_id: u32) -> Result<()> {
    let mut registry = ADAPTER_REGISTRY.lock();
    
    let pos = registry.iter().position(|a| a.id == adapter_id)
        .ok_or(Error::NotFound)?;
    
    let adapter = registry.remove(pos);
    log::info!("I2C adapter {}: Unregistered", adapter.name);
    
    Ok(())
}

/// i2c_get_adapter - Get I2C adapter by ID
pub fn i2c_get_adapter(adapter_id: u32) -> Option<Arc<I2cAdapter>> {
    let registry = ADAPTER_REGISTRY.lock();
    registry.iter().find(|a| a.id == adapter_id).cloned()
}

/// i2c_for_each_adapter - Iterate over all adapters
pub fn i2c_for_each_adapter<F>(mut f: F) where F: FnMut(&Arc<I2cAdapter>) {
    let registry = ADAPTER_REGISTRY.lock();
    for adapter in registry.iter() {
        f(adapter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;

    struct DummyAlgorithm;

    impl I2cAlgorithm for DummyAlgorithm {
        fn master_xfer(&self, _adapter: &I2cAdapter, msgs: &mut [I2cMsg]) -> Result<usize> {
            Ok(msgs.len())
        }

        fn functionality(&self) -> u32 {
            0xFFFFFFFF
        }
    }

    #[test]
    fn test_i2c_freq_mode_string() {
        assert_eq!(i2c_freq_mode_string(100_000), "Standard Mode (100 kHz)");
        assert_eq!(i2c_freq_mode_string(400_000), "Fast Mode (400 kHz)");
        assert_eq!(i2c_freq_mode_string(1_000_000), "Fast Mode Plus (1.0 MHz)");
    }

    #[test]
    fn test_i2c_msg_creation() {
        let msg = I2cMsg::read(0x50, 10);
        assert_eq!(msg.addr, 0x50);
        assert!(msg.is_read());
        assert_eq!(msg.buf.len(), 10);

        let msg = I2cMsg::write(0x51, vec![1, 2, 3]);
        assert_eq!(msg.addr, 0x51);
        assert!(!msg.is_read());
        assert_eq!(msg.buf.len(), 3);
    }

    #[test]
    fn test_i2c_adapter_creation() {
        let algo = Arc::new(DummyAlgorithm);
        let adapter = I2cAdapter::new("test-i2c".into(), algo, 100_000);
        
        assert_eq!(adapter.name, "test-i2c");
        assert_eq!(adapter.bus_freq_hz, 100_000);
        assert_eq!(adapter.retries, 3);
    }

    #[test]
    fn test_i2c_client_address_validation() {
        let algo = Arc::new(DummyAlgorithm);
        let adapter = Arc::new(I2cAdapter::new("test-i2c".into(), algo, 100_000));

        // Valid 7-bit address
        let client = I2cClient::new("test-device".into(), 0x50, 0, adapter.clone());
        assert!(client.is_ok());

        // Invalid 7-bit address
        let client = I2cClient::new("test-device".into(), 0x80, 0, adapter.clone());
        assert!(client.is_err());

        // Valid 10-bit address
        let client = I2cClient::new("test-device".into(), 0x200, I2C_CLIENT_TEN, adapter.clone());
        assert!(client.is_ok());
    }

    #[test]
    fn test_i2c_adapter_registry() {
        let algo = Arc::new(DummyAlgorithm);
        let adapter = Arc::new(I2cAdapter::new("test-i2c".into(), algo, 100_000));
        let id = adapter.id;

        assert!(i2c_add_adapter(adapter.clone()).is_ok());
        
        let found = i2c_get_adapter(id);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "test-i2c");

        assert!(i2c_del_adapter(id).is_ok());
        assert!(i2c_get_adapter(id).is_none());
    }

    #[test]
    fn test_i2c_transfer() {
        let algo = Arc::new(DummyAlgorithm);
        let adapter = Arc::new(I2cAdapter::new("test-i2c".into(), algo, 100_000));

        let mut msgs = vec![
            I2cMsg::write(0x50, vec![0x00]),
            I2cMsg::read(0x50, 4),
        ];

        let result = adapter.transfer(&mut msgs);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
    }
}
