// SPDX-License-Identifier: MIT OR Apache-2.0
//! SPI Subsystem
//!
//! Complete SPI subsystem ported from Linux kernel

pub mod core;
pub mod of;
pub mod bitbang;

pub use core::{
    SpiController, SpiDevice, SpiTransfer, SpiMessage,
    SpiControllerOps, SpiBoardInfo,
    spi_register_controller, spi_unregister_controller, spi_get_controller,
    spi_alloc_device, spi_setup, spi_sync, spi_async,
    spi_write_then_read, spi_register_board_info,
    SPI_MODE_0, SPI_MODE_1, SPI_MODE_2, SPI_MODE_3,
    SPI_CPHA, SPI_CPOL, SPI_CS_HIGH, SPI_LSB_FIRST,
    SPI_3WIRE, SPI_LOOP, SPI_NO_CS,
    SPI_TX_DUAL, SPI_TX_QUAD, SPI_RX_DUAL, SPI_RX_QUAD,
};

pub use of::{
    of_spi_parse_dt, of_register_spi_device, of_register_spi_devices,
};

pub use bitbang::{
    SpiBitbang, SpiBitbangCs, BitbangCsState,
    spi_bitbang_setup, spi_bitbang_setup_transfer,
    spi_bitbang_cleanup, spi_bitbang_init,
};
