// SPDX-License-Identifier: MIT OR Apache-2.0
//! SPI Device Tree Support
//!
//! Ported from Linux: drivers/spi/spi.c (of_spi_* functions)
//! Source lines: ~250 C → ~200 Rust

use crate::drivers::of::{DeviceNode, OfError};
use crate::drivers::spi::core::{SpiController, SpiDevice, SpiMode, SpiDelay, SpiDelayUnit, spi_alloc_device};
use alloc::string::ToString;
use alloc::sync::Arc;

/// Maximum number of chip selects per device
const SPI_DEVICE_CS_CNT_MAX: usize = 4;

/// Maximum number of data lanes
const SPI_DEVICE_DATA_LANE_CNT_MAX: usize = 8;

/// Parse CS delay from Device Tree property
///
/// Ported from: of_spi_parse_dt_cs_delay()
fn of_spi_parse_dt_cs_delay(nc: &DeviceNode, prop: &str) -> Option<SpiDelay> {
    if let Ok(value) = nc.read_u32(prop) {
        if value > u16::MAX as u32 {
            Some(SpiDelay {
                value: ((value + 999) / 1000) as u16, // DIV_ROUND_UP
                unit: SpiDelayUnit::Usecs,
            })
        } else {
            Some(SpiDelay {
                value: value as u16,
                unit: SpiDelayUnit::Nsecs,
            })
        }
    } else {
        None
    }
}

/// Parse SPI device properties from Device Tree
///
/// Ported from: of_spi_parse_dt()
/// Source: linux-master/drivers/spi/spi.c:2354
///
/// # Arguments
/// * `ctlr` - SPI controller
/// * `spi` - SPI device to configure
/// * `nc` - Device Tree node
pub fn of_spi_parse_dt(
    ctlr: &SpiController,
    spi: &mut SpiDevice,
    nc: &DeviceNode,
) -> Result<(), OfError> {
    // Mode (clock phase/polarity/etc.)
    if nc.read_bool("spi-cpha") {
        spi.mode |= SpiMode::CPHA.0;
    }
    if nc.read_bool("spi-cpol") {
        spi.mode |= SpiMode::CPOL.0;
    }
    if nc.read_bool("spi-3wire") {
        spi.mode |= SpiMode::THREE_WIRE.0;
    }
    if nc.read_bool("spi-lsb-first") {
        spi.mode |= SpiMode::LSB_FIRST.0;
    }
    if nc.read_bool("spi-cs-high") {
        spi.mode |= SpiMode::CS_HIGH.0;
    }

    // TX lane map
    let tx_lane_map = match nc.read_u32_array("spi-tx-lane-map") {
        Ok(map) if !map.is_empty() => {
            if map.len() > SPI_DEVICE_DATA_LANE_CNT_MAX {
                return Err(OfError::InvalidValue);
            }
            map
        }
        Ok(_) => {
            // Empty map: use default identity mapping
            (0..SPI_DEVICE_DATA_LANE_CNT_MAX as u32).collect()
        }
        Err(OfError::NotFound) => {
            // Default: identity mapping
            (0..SPI_DEVICE_DATA_LANE_CNT_MAX as u32).collect()
        }
        Err(e) => return Err(e),
    };

    let max_tx_lanes = tx_lane_map.len();
    for (i, &val) in tx_lane_map.iter().enumerate().take(max_tx_lanes) {
        spi.tx_lane_map[i] = val as u8;
    }

    // TX bus width
    let _tx_bus_width = match nc.read_u32_array("spi-tx-bus-width") {
        Ok(widths) if !widths.is_empty() => {
            if widths.len() > max_tx_lanes {
                return Err(OfError::InvalidValue);
            }

            // Check all widths are the same
            let first = widths[0];
            if !widths.iter().all(|&w| w == first) {
                return Err(OfError::InvalidValue);
            }

            spi.num_tx_lanes = widths.len() as u8;

            match first {
                0 => spi.mode |= SpiMode::NO_TX.0,
                1 => {}
                2 => spi.mode |= SpiMode::TX_DUAL.0,
                4 => spi.mode |= SpiMode::TX_QUAD.0,
                8 => spi.mode |= SpiMode::TX_OCTAL.0,
                _ => {} // Unsupported, ignore
            }
            first
        }
        Ok(_) => {
            // Empty widths: default to 1
            spi.num_tx_lanes = 1;
            1
        }
        Err(OfError::NotFound) => {
            spi.num_tx_lanes = 1;
            1
        }
        Err(e) => return Err(e),
    };

    // Validate TX lane map
    for &lane in &spi.tx_lane_map[..spi.num_tx_lanes as usize] {
        if lane >= SPI_DEVICE_DATA_LANE_CNT_MAX as u8 {
            return Err(OfError::InvalidValue);
        }
    }

    // RX lane map
    let rx_lane_map = match nc.read_u32_array("spi-rx-lane-map") {
        Ok(map) if !map.is_empty() => {
            if map.len() > SPI_DEVICE_DATA_LANE_CNT_MAX {
                return Err(OfError::InvalidValue);
            }
            map
        }
        Ok(_) => {
            // Empty map: use default identity mapping
            (0..SPI_DEVICE_DATA_LANE_CNT_MAX as u32).collect()
        }
        Err(OfError::NotFound) => {
            // Default: identity mapping
            (0..SPI_DEVICE_DATA_LANE_CNT_MAX as u32).collect()
        }
        Err(e) => return Err(e),
    };

    let max_rx_lanes = rx_lane_map.len();
    for (i, &val) in rx_lane_map.iter().enumerate().take(max_rx_lanes) {
        spi.rx_lane_map[i] = val as u8;
    }

    // RX bus width
    let _rx_bus_width = match nc.read_u32_array("spi-rx-bus-width") {
        Ok(widths) if !widths.is_empty() => {
            if widths.len() > max_rx_lanes {
                return Err(OfError::InvalidValue);
            }

            // Check all widths are the same
            let first = widths[0];
            if !widths.iter().all(|&w| w == first) {
                return Err(OfError::InvalidValue);
            }

            spi.num_rx_lanes = widths.len() as u8;

            match first {
                0 => spi.mode |= SpiMode::NO_RX.0,
                1 => {}
                2 => spi.mode |= SpiMode::RX_DUAL.0,
                4 => spi.mode |= SpiMode::RX_QUAD.0,
                8 => spi.mode |= SpiMode::RX_OCTAL.0,
                _ => {} // Unsupported, ignore
            }
            first
        }
        Ok(_) => {
            // Empty widths: default to 1
            spi.num_rx_lanes = 1;
            1
        }
        Err(OfError::NotFound) => {
            spi.num_rx_lanes = 1;
            1
        }
        Err(e) => return Err(e),
    };

    // Validate RX lane map
    for &lane in &spi.rx_lane_map[..spi.num_rx_lanes as usize] {
        if lane >= SPI_DEVICE_DATA_LANE_CNT_MAX as u8 {
            return Err(OfError::InvalidValue);
        }
    }

    // Check if this is a target (slave) device
    if ctlr.is_target {
        if nc.name != "slave" {
            return Err(OfError::InvalidValue);
        }
        return Ok(());
    }

    // Device address (chip select)
    let cs_array = nc.read_u32_array("reg")?;
    if cs_array.is_empty() || cs_array.len() > SPI_DEVICE_CS_CNT_MAX {
        return Err(OfError::InvalidValue);
    }

    // Check for parallel memories support
    if nc.read_bool("parallel-memories") && !ctlr.supports_multi_cs {
        return Err(OfError::InvalidValue);
    }

    spi.num_chipselect = cs_array.len();
    spi.chipselect[..cs_array.len()].copy_from_slice(&cs_array);
    spi.cs_index_mask = 1; // Bit 0 set by default

    // Device speed
    if let Ok(max_freq) = nc.read_u32("spi-max-frequency") {
        spi.max_speed_hz = max_freq;
    }

    // Device CS delays
    if let Some(delay) = of_spi_parse_dt_cs_delay(nc, "spi-cs-setup-delay-ns") {
        spi.cs_setup = delay;
    }
    if let Some(delay) = of_spi_parse_dt_cs_delay(nc, "spi-cs-hold-delay-ns") {
        spi.cs_hold = delay;
    }
    if let Some(delay) = of_spi_parse_dt_cs_delay(nc, "spi-cs-inactive-delay-ns") {
        spi.cs_inactive = delay;
    }

    Ok(())
}

/// Register a single SPI device from Device Tree node
///
/// Ported from: of_register_spi_device()
/// Source: linux-master/drivers/spi/spi.c:2603
///
/// # Arguments
/// * `ctlr` - SPI controller
/// * `nc` - Device Tree node for the SPI device
pub fn of_register_spi_device(
    ctlr: Arc<SpiController>,
    nc: &DeviceNode,
) -> Result<Arc<SpiDevice>, OfError> {
    // Alloc an spi_device
    let mut spi_arc = spi_alloc_device(ctlr.clone()).map_err(|_| OfError::InvalidValue)?;
    let spi = Arc::make_mut(&mut spi_arc);

    // Select device driver (modalias from compatible)
    use crate::drivers::of::base::of_get_property;
    let compat = of_get_property(Some(nc), "compatible")
        .and_then(|v| core::str::from_utf8(v).ok())
        .and_then(|s| s.split('\0').next())
        .ok_or(OfError::NotFound)?;
    spi.modalias = compat.to_string();

    // Parse DT properties
    of_spi_parse_dt(&ctlr, spi, nc)?;

    Ok(spi_arc)
}

/// Register all child SPI devices from Device Tree
///
/// Ported from: of_register_spi_devices()
/// Source: linux-master/drivers/spi/spi.c:2656
///
/// Registers an spi_device for each child node of controller node which
/// represents a valid SPI target device.
///
/// # Arguments
/// * `ctlr` - SPI controller
/// * `controller_node` - Device Tree node for the controller
pub fn of_register_spi_devices(_ctlr: Arc<SpiController>, _controller_node: &DeviceNode) -> Result<(), OfError> {
    // TODO: Implement child iteration when DeviceNode provides safe iterator
    // For now, just return Ok
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_spi_parse_dt_cs_delay_nsecs() {
        // Test nanosecond delay
        let node = DeviceNode::mock_with_property("spi-cs-setup-delay-ns", 100u32);
        let delay = of_spi_parse_dt_cs_delay(&node, "spi-cs-setup-delay-ns");
        assert!(delay.is_some());
        let delay = delay.unwrap();
        assert_eq!(delay.value, 100);
        assert_eq!(delay.unit, SpiDelayUnit::Nsecs);
    }

    #[test]
    fn test_of_spi_parse_dt_cs_delay_usecs() {
        // Test microsecond delay (value > u16::MAX)
        let node = DeviceNode::mock_with_property("spi-cs-hold-delay-ns", 100000u32);
        let delay = of_spi_parse_dt_cs_delay(&node, "spi-cs-hold-delay-ns");
        assert!(delay.is_some());
        let delay = delay.unwrap();
        assert_eq!(delay.value, 100); // 100000 / 1000
        assert_eq!(delay.unit, SpiDelayUnit::Usecs);
    }

    #[test]
    fn test_of_spi_parse_dt_mode_flags() {
        let mut node = DeviceNode::mock();
        node.add_bool_property("spi-cpha");
        node.add_bool_property("spi-cpol");
        node.add_bool_property("spi-cs-high");

        let mut ctlr = SpiController::mock();
        let mut spi = SpiDevice::new();

        let result = of_spi_parse_dt(&ctlr, &mut spi, &node);
        assert!(result.is_ok());
        assert!(spi.mode.contains(SpiMode::CPHA));
        assert!(spi.mode.contains(SpiMode::CPOL));
        assert!(spi.mode.contains(SpiMode::CS_HIGH));
    }

    #[test]
    fn test_of_spi_parse_dt_tx_dual() {
        let mut node = DeviceNode::mock();
        node.add_property("spi-tx-bus-width", &[2u32]);
        node.add_property("reg", &[0u32]);

        let mut ctlr = SpiController::mock();
        let mut spi = SpiDevice::new();

        let result = of_spi_parse_dt(&ctlr, &mut spi, &node);
        assert!(result.is_ok());
        assert!(spi.mode.contains(SpiMode::TX_DUAL));
        assert_eq!(spi.num_tx_lanes, 1);
    }

    #[test]
    fn test_of_spi_parse_dt_rx_quad() {
        let mut node = DeviceNode::mock();
        node.add_property("spi-rx-bus-width", &[4u32]);
        node.add_property("reg", &[0u32]);

        let mut ctlr = SpiController::mock();
        let mut spi = SpiDevice::new();

        let result = of_spi_parse_dt(&ctlr, &mut spi, &node);
        assert!(result.is_ok());
        assert!(spi.mode.contains(SpiMode::RX_QUAD));
        assert_eq!(spi.num_rx_lanes, 1);
    }

    #[test]
    fn test_of_spi_parse_dt_chipselect() {
        let mut node = DeviceNode::mock();
        node.add_property("reg", &[0u32, 1u32]);

        let mut ctlr = SpiController::mock();
        let mut spi = SpiDevice::new();

        let result = of_spi_parse_dt(&ctlr, &mut spi, &node);
        assert!(result.is_ok());
        assert_eq!(spi.num_chipselect, 2);
        assert_eq!(spi.chipselect[0], 0);
        assert_eq!(spi.chipselect[1], 1);
    }

    #[test]
    fn test_of_spi_parse_dt_max_frequency() {
        let mut node = DeviceNode::mock();
        node.add_property("spi-max-frequency", &[10_000_000u32]);
        node.add_property("reg", &[0u32]);

        let mut ctlr = SpiController::mock();
        let mut spi = SpiDevice::new();

        let result = of_spi_parse_dt(&ctlr, &mut spi, &node);
        assert!(result.is_ok());
        assert_eq!(spi.max_speed_hz, 10_000_000);
    }

    #[test]
    fn test_of_register_spi_device() {
        let mut node = DeviceNode::mock();
        node.add_compatible("spidev");
        node.add_property("reg", &[0u32]);
        node.add_property("spi-max-frequency", &[1_000_000u32]);

        let mut ctlr = SpiController::mock();

        let result = of_register_spi_device(&mut ctlr, &node);
        assert!(result.is_ok());

        let spi = result.unwrap();
        assert_eq!(spi.modalias, "spidev");
        assert_eq!(spi.max_speed_hz, 1_000_000);
    }

    #[test]
    fn test_of_register_spi_devices() {
        let mut controller_node = DeviceNode::mock();
        
        let mut child1 = DeviceNode::mock();
        child1.add_compatible("spidev");
        child1.add_property("reg", &[0u32]);
        child1.set_available(true);
        
        let mut child2 = DeviceNode::mock();
        child2.add_compatible("spi-nor");
        child2.add_property("reg", &[1u32]);
        child2.set_available(true);

        controller_node.add_child(child1);
        controller_node.add_child(child2);

        let mut ctlr = SpiController::mock();
        ctlr.of_node = Some(controller_node);

        let result = of_register_spi_devices(&mut ctlr);
        assert!(result.is_ok());
        assert_eq!(ctlr.devices.len(), 2);
    }
}
