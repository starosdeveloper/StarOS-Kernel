// SPDX-License-Identifier: MIT OR Apache-2.0
//! I2C Subsystem
//!
//! Complete I2C subsystem ported from Linux kernel
//! Provides I2C adapter and client management

pub mod core;
pub mod algo;
pub mod of;

pub use core::{
    I2cAdapter, I2cClient, I2cMsg, I2cAlgorithm, I2cBusRecoveryInfo,
    i2c_add_adapter, i2c_del_adapter, i2c_get_adapter, i2c_for_each_adapter,
    i2c_generic_scl_recovery, i2c_freq_mode_string,
    I2C_MAX_STANDARD_MODE_FREQ, I2C_MAX_FAST_MODE_FREQ, I2C_MAX_FAST_MODE_PLUS_FREQ,
    I2C_CLIENT_TEN, I2C_CLIENT_SLAVE, I2C_M_RD, I2C_M_TEN,
};

pub use algo::{I2cAlgoBitData, I2cBitOps};

pub use of::{
    I2cBoardInfo, I2cOfMatch,
    of_i2c_get_board_info, of_i2c_register_device, of_i2c_register_devices,
    of_find_i2c_device_by_node, of_find_i2c_adapter_by_node,
    i2c_of_match_device,
};
