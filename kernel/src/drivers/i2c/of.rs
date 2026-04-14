// SPDX-License-Identifier: MIT OR Apache-2.0
//! I2C Device Tree Support
//!
//! Ported from: linux-master/drivers/i2c/i2c-core-of.c
//! Source lines: 218 C → 300 Rust
//!
//! This module implements Device Tree support for I2C subsystem:
//! - Parsing I2C device nodes from Device Tree
//! - Automatic device registration from DT
//! - Device matching and probing
//!
//! Copyright (C) 2008 Jochen Friedrich <jochen@scram.de>
//! Copyright (C) 2013, 2018 Wolfram Sang <wsa@kernel.org>

use alloc::sync::Arc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::drivers::i2c::core::{I2cAdapter, I2cClient, Error, Result};
use crate::drivers::i2c::core::{I2C_CLIENT_TEN, I2C_CLIENT_SLAVE, I2C_CLIENT_HOST_NOTIFY, I2C_CLIENT_WAKE};
use crate::drivers::of::base::DeviceNode;
use crate::drivers::of::property::{of_property_read_u32, of_property_read_bool, of_property_read_string};

/// I2C Device Tree address flags (from dt-bindings/i2c/i2c.h)
const I2C_TEN_BIT_ADDRESS: u32 = 0x80000000;
const I2C_OWN_SLAVE_ADDRESS: u32 = 0x40000000;

/// I2C board info structure
///
/// Contains information needed to instantiate an I2C device
#[derive(Debug, Clone)]
pub struct I2cBoardInfo {
    /// Device type/modalias
    pub device_type: String,
    /// I2C address
    pub addr: u16,
    /// Client flags
    pub flags: u16,
}

impl I2cBoardInfo {
    /// Create new board info
    pub fn new() -> Self {
        Self {
            device_type: String::new(),
            addr: 0,
            flags: 0,
        }
    }
}

/// of_i2c_get_board_info - Extract I2C board info from Device Tree node
///
/// Ported from: of_i2c_get_board_info()
/// Source: linux-master/drivers/i2c/i2c-core-of.c:22
///
/// # Arguments
/// * `node` - Device Tree node to parse
///
/// # Returns
/// I2C board info structure on success
pub fn of_i2c_get_board_info(node: &DeviceNode) -> Result<I2cBoardInfo> {
    let mut info = I2cBoardInfo::new();

    // Get device type from compatible property (of_alias_from_compatible equivalent)
    match of_property_read_string(Some(node), "compatible") {
        Ok(compatible) => {
            // Extract device type from compatible string
            // Format: "vendor,device" -> use "device" part
            if let Some(comma_pos) = compatible.find(',') {
                info.device_type = compatible[comma_pos + 1..].to_string();
            } else {
                info.device_type = compatible.to_string();
            }
        }
        Err(_) => {
            log::error!("of_i2c: modalias failure on {}", node.name);
            return Err(Error::InvalidArgument);
        }
    }

    // Read I2C address from "reg" property
    let mut addr = of_property_read_u32(Some(node), "reg")
        .map_err(|_| {
            log::error!("of_i2c: invalid reg on {}", node.name);
            Error::InvalidArgument
        })?;

    // Check for 10-bit addressing flag
    if addr & I2C_TEN_BIT_ADDRESS != 0 {
        addr &= !I2C_TEN_BIT_ADDRESS;
        info.flags |= I2C_CLIENT_TEN;
    }

    // Check for slave mode flag
    if addr & I2C_OWN_SLAVE_ADDRESS != 0 {
        addr &= !I2C_OWN_SLAVE_ADDRESS;
        info.flags |= I2C_CLIENT_SLAVE;
    }

    // Validate address range
    if info.flags & I2C_CLIENT_TEN != 0 {
        if addr > 0x3ff {
            log::error!("of_i2c: invalid 10-bit address 0x{:x}", addr);
            return Err(Error::InvalidArgument);
        }
    } else if addr > 0x7f {
        log::error!("of_i2c: invalid 7-bit address 0x{:x}", addr);
        return Err(Error::InvalidArgument);
    }

    info.addr = addr as u16;

    // Check for host-notify property
    if of_property_read_bool(Some(node), "host-notify") {
        info.flags |= I2C_CLIENT_HOST_NOTIFY;
    }

    // Check for wakeup-source property
    if of_property_read_bool(Some(node), "wakeup-source") {
        info.flags |= I2C_CLIENT_WAKE;
    }

    log::debug!("of_i2c: parsed device {} at address 0x{:02x}, flags=0x{:04x}",
               info.device_type, info.addr, info.flags);

    Ok(info)
}

/// of_i2c_register_device - Register single I2C device from Device Tree node
///
/// Ported from: of_i2c_register_device()
/// Source: linux-master/drivers/i2c/i2c-core-of.c:65
///
/// # Arguments
/// * `adapter` - I2C adapter to register device on
/// * `node` - Device Tree node describing the device
///
/// # Returns
/// Registered I2C client on success
pub fn of_i2c_register_device(adapter: &Arc<I2cAdapter>, node: &DeviceNode) -> Result<Arc<I2cClient>> {
    log::debug!("of_i2c: register {}", node.name);

    // Get board info from Device Tree node
    let info = of_i2c_get_board_info(node)?;

    // Create new I2C client device (i2c_new_client_device equivalent)
    let client = I2cClient::new(
        info.device_type.clone(),
        info.addr,
        info.flags,
        adapter.clone(),
    ).map_err(|e| {
        log::error!("of_i2c: Failure registering {}", node.name);
        e
    })?;

    log::info!("of_i2c: registered device {} at 0x{:02x} on adapter {}",
              info.device_type, info.addr, adapter.name);

    Ok(client)
}

/// of_i2c_register_devices - Register all I2C devices from Device Tree
///
/// Ported from: of_i2c_register_devices()
/// Source: linux-master/drivers/i2c/i2c-core-of.c:85
///
/// This function walks the Device Tree and registers all I2C devices
/// found as children of the adapter's device node.
///
/// # Arguments
/// * `adapter` - I2C adapter to register devices on
/// * `adapter_node` - Device Tree node of the adapter
/// * `child_nodes` - Vector of child device nodes to register
///
/// # Returns
/// Vector of registered I2C clients
pub fn of_i2c_register_devices(
    adapter: &Arc<I2cAdapter>,
    adapter_node: &DeviceNode,
    child_nodes: &[DeviceNode]
) -> Result<Vec<Arc<I2cClient>>> {
    let mut clients = Vec::new();

    log::debug!("of_i2c: walking child nodes of {}", adapter_node.name);

    // Iterate over all available child nodes (for_each_available_child_of_node equivalent)
    for node in child_nodes {
        // Skip if already populated (of_node_test_and_set_flag equivalent)
        // In full implementation would check OF_POPULATED flag
        
        // Try to register device from this node
        match of_i2c_register_device(adapter, node) {
            Ok(client) => {
                log::info!("of_i2c: successfully registered device from {}", node.name);
                clients.push(client);
            }
            Err(e) => {
                log::error!("of_i2c: Failed to create I2C device for {}: {:?}", node.name, e);
                // In full implementation would clear OF_POPULATED flag
            }
        }
    }

    log::info!("of_i2c: registered {} devices on adapter {}", clients.len(), adapter.name);

    Ok(clients)
}

/// I2C Device Tree matching support
#[derive(Debug, Clone)]
pub struct I2cOfMatch {
    /// Compatible string
    pub compatible: String,
    /// Driver data (optional)
    pub data: usize,
}

impl I2cOfMatch {
    /// Create new OF match entry
    pub fn new(compatible: &str) -> Self {
        Self {
            compatible: compatible.to_string(),
            data: 0,
        }
    }

    /// Create new OF match entry with data
    pub fn with_data(compatible: &str, data: usize) -> Self {
        Self {
            compatible: compatible.to_string(),
            data,
        }
    }
}

/// i2c_of_match_device_sysfs - Match device via sysfs interface
///
/// Ported from: i2c_of_match_device_sysfs()
/// Source: linux-master/drivers/i2c/i2c-core-of.c:117
///
/// Adding devices through the i2c sysfs interface provides us
/// a string to match which may be compatible with the device
/// tree compatible strings, however with no actual of_node the
/// of_match_device() will not match.
///
/// # Arguments
/// * `matches` - Array of OF device ID matches
/// * `client_name` - Client name from sysfs
///
/// # Returns
/// Matching entry if found
fn i2c_of_match_device_sysfs<'a>(matches: &'a [I2cOfMatch], client_name: &str) -> Option<&'a I2cOfMatch> {
    for m in matches {
        // Check if compatible string is empty (end of table)
        if m.compatible.is_empty() {
            break;
        }

        // Direct match with compatible string (sysfs_streq equivalent)
        if client_name == m.compatible {
            return Some(m);
        }

        // Match without vendor prefix (after comma)
        let name = if let Some(comma_pos) = m.compatible.find(',') {
            &m.compatible[comma_pos + 1..]
        } else {
            &m.compatible
        };

        if client_name == name {
            return Some(m);
        }
    }

    None
}

/// i2c_of_match_device - Match I2C device against OF match table
///
/// Ported from: i2c_of_match_device()
/// Source: linux-master/drivers/i2c/i2c-core-of.c:148
///
/// # Arguments
/// * `matches` - Array of OF device ID matches
/// * `client_name` - I2C client name to match
///
/// # Returns
/// Matching entry if found
pub fn i2c_of_match_device<'a>(matches: &'a [I2cOfMatch], client_name: &str) -> Option<&'a I2cOfMatch> {
    if matches.is_empty() {
        return None;
    }

    // In full Linux implementation, would first try of_match_device()
    // which matches against actual device tree node
    // Since we don't have device node attached to client here,
    // we fall back to sysfs matching

    // Try sysfs matching
    i2c_of_match_device_sysfs(matches, client_name)
}

/// of_find_i2c_device_by_node - Find I2C device by Device Tree node
///
/// Ported from: of_find_i2c_device_by_node()
/// Source: linux-master/drivers/i2c/i2c-core-of.c (referenced in of_i2c_notify)
///
/// # Arguments
/// * `node` - Device Tree node to find
///
/// # Returns
/// I2C client address if found
pub fn of_find_i2c_device_by_node(node: &DeviceNode) -> Option<u16> {
    // Get address from node
    match of_property_read_u32(Some(node), "reg") {
        Ok(addr) => Some((addr & 0x7f) as u16),
        Err(_) => None,
    }
}

/// of_find_i2c_adapter_by_node - Find I2C adapter by Device Tree node
///
/// Ported from: of_find_i2c_adapter_by_node()
/// Source: linux-master/drivers/i2c/i2c-core-of.c (referenced in of_i2c_notify)
///
/// # Arguments
/// * `node_name` - Device Tree node name to find
///
/// # Returns
/// I2C adapter if found
pub fn of_find_i2c_adapter_by_node(node_name: &str) -> Option<Arc<I2cAdapter>> {
    use crate::drivers::i2c::core::i2c_for_each_adapter;
    
    let mut found = None;
    i2c_for_each_adapter(|adapter| {
        // In full implementation would compare device node pointers
        // Here we compare by name as simplified version
        if adapter.name == node_name {
            found = Some(adapter.clone());
        }
    });
    
    found
}

// Note: of_i2c_notify() and CONFIG_OF_DYNAMIC support not implemented
// as it requires full device tree dynamic reconfiguration support
// which is not yet available in StarOS kernel

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_info_creation() {
        let info = I2cBoardInfo::new();
        assert_eq!(info.addr, 0);
        assert_eq!(info.flags, 0);
        assert!(info.device_type.is_empty());
    }

    #[test]
    fn test_of_match_creation() {
        let m = I2cOfMatch::new("vendor,device");
        assert_eq!(m.compatible, "vendor,device");
        assert_eq!(m.data, 0);

        let m = I2cOfMatch::with_data("vendor,device", 0x1234);
        assert_eq!(m.data, 0x1234);
    }

    #[test]
    fn test_address_flags() {
        // Test 10-bit address flag
        assert_eq!(I2C_TEN_BIT_ADDRESS, 0x80000000);
        assert_eq!(I2C_OWN_SLAVE_ADDRESS, 0x40000000);
    }

    #[test]
    fn test_of_match_device_sysfs() {
        let matches = vec![
            I2cOfMatch::new("vendor,device1"),
            I2cOfMatch::new("vendor,device2"),
        ];

        // Test direct match
        assert!(i2c_of_match_device_sysfs(&matches, "vendor,device1").is_some());
        
        // Test match without vendor prefix
        assert!(i2c_of_match_device_sysfs(&matches, "device2").is_some());
        
        // Test no match
        assert!(i2c_of_match_device_sysfs(&matches, "nonexistent").is_none());
    }

    #[test]
    fn test_i2c_of_match_device() {
        let matches = vec![
            I2cOfMatch::new("vendor,eeprom"),
            I2cOfMatch::new("vendor,sensor"),
        ];

        // Test match
        assert!(i2c_of_match_device(&matches, "eeprom").is_some());
        assert!(i2c_of_match_device(&matches, "vendor,sensor").is_some());
        
        // Test no match
        assert!(i2c_of_match_device(&matches, "unknown").is_none());
    }
}
