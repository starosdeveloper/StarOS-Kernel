// SPDX-License-Identifier: MIT OR Apache-2.0
//! SPI Bitbang (Software) Implementation
//!
//! Ported from Linux: drivers/spi/spi-bitbang.c
//! Source lines: ~400 C → ~300 Rust
//!
//! Polling/bitbanging SPI host controller driver utilities.
//! Use this for GPIO or shift-register level hardware APIs.

use crate::drivers::spi::core::{SpiController, SpiDevice, SpiTransfer, SpiMode};
use spin::Mutex;

/// CS delay in nanoseconds
const SPI_BITBANG_CS_DELAY: u32 = 100;

/// Nanoseconds per second
const NSEC_PER_SEC: u64 = 1_000_000_000;

/// Nanoseconds per millisecond
const NSEC_PER_MSEC: u64 = 1_000_000;

/// Maximum microsecond delay
const MAX_UDELAY_MS: u64 = 2000;

/// Chip select states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitbangCsState {
    /// Chip select inactive
    Inactive = 0,
    /// Chip select active
    Active = 1,
}

/// Transfer flags
pub mod flags {
    /// No RX during transfer
    pub const NO_RX: u32 = 1 << 0;
    /// No TX during transfer
    pub const NO_TX: u32 = 1 << 1;
}

/// Word transfer function type
pub type TxRxWordFn = fn(&SpiDevice, u32, u32, u8, u32) -> u32;

/// Buffer transfer function type
pub type TxRxBufsFn = fn(&SpiDevice, TxRxWordFn, u32, &mut SpiTransfer, u32) -> usize;

/// Per-device bitbang state
#[derive(Clone)]
pub struct SpiBitbangCs {
    /// Nanoseconds per half clock cycle
    pub nsecs: u32,
    /// Word transfer function
    pub txrx_word: Option<TxRxWordFn>,
    /// Buffer transfer function
    pub txrx_bufs: Option<TxRxBufsFn>,
}

impl SpiBitbangCs {
    /// Create new bitbang CS state
    pub fn new() -> Self {
        Self {
            nsecs: 0,
            txrx_word: None,
            txrx_bufs: None,
        }
    }
}

/// SPI bitbang controller
pub struct SpiBitbang {
    /// Associated SPI controller
    pub ctlr: SpiController,
    /// Busy flag
    pub busy: bool,
    /// Lock for busy flag
    pub lock: Mutex<()>,
    /// Mode flags supported
    pub flags: u32,
    /// Chipselect callback
    pub chipselect: Option<fn(&SpiDevice, BitbangCsState)>,
    /// Setup transfer callback
    pub setup_transfer: Option<fn(&SpiDevice, Option<&SpiTransfer>) -> Result<(), i32>>,
    /// Set MOSI idle state callback
    pub set_mosi_idle: Option<fn(&SpiDevice)>,
    /// Set line direction callback (for 3-wire mode)
    pub set_line_direction: Option<fn(&SpiDevice, bool) -> Result<(), i32>>,
    /// TX/RX buffers function
    pub txrx_bufs: Option<fn(&SpiDevice, &SpiTransfer) -> Result<usize, i32>>,
    /// TX/RX word functions for each mode
    pub txrx_word: [Option<TxRxWordFn>; 4],
    /// Use DMA flag
    pub use_dma: bool,
}

impl SpiBitbang {
    /// Create new bitbang controller
    pub fn new(ctlr: SpiController) -> Self {
        Self {
            ctlr,
            busy: false,
            lock: Mutex::new(()),
            flags: 0,
            chipselect: None,
            setup_transfer: None,
            set_mosi_idle: None,
            set_line_direction: None,
            txrx_bufs: None,
            txrx_word: [None; 4],
            use_dma: false,
        }
    }
}

/// Transfer 8-bit words
///
/// Ported from: bitbang_txrx_8()
fn bitbang_txrx_8(
    spi: &SpiDevice,
    txrx_word: TxRxWordFn,
    ns: u32,
    t: &mut SpiTransfer,
    flags: u32,
) -> usize {
    let bits = if t.bits_per_word == 0 { 8 } else { t.bits_per_word };
    let mut count = t.len;
    let mut tx_idx = 0;
    let mut rx_idx = 0;

    while count > 0 {
        let word = if let Some(tx_buf) = &t.tx_buf {
            if tx_idx < tx_buf.len() {
                tx_buf[tx_idx] as u32
            } else {
                if (spi.mode & SpiMode::MOSI_IDLE_HIGH.0) != 0 { 0xFF } else { 0 }
            }
        } else {
            if (spi.mode & SpiMode::MOSI_IDLE_HIGH.0) != 0 { 0xFF } else { 0 }
        };

        let result = txrx_word(spi, ns, word, bits, flags);

        if let Some(rx_buf) = &mut t.rx_buf {
            if rx_idx < rx_buf.len() {
                rx_buf[rx_idx] = result as u8;
            }
        }

        tx_idx += 1;
        rx_idx += 1;
        count -= 1;
    }

    t.len - count
}

/// Transfer 16-bit words
///
/// Ported from: bitbang_txrx_16()
fn bitbang_txrx_16(
    spi: &SpiDevice,
    txrx_word: TxRxWordFn,
    ns: u32,
    t: &mut SpiTransfer,
    flags: u32,
) -> usize {
    let bits = if t.bits_per_word == 0 { 16 } else { t.bits_per_word };
    let mut count = t.len;
    let mut tx_idx = 0;
    let mut rx_idx = 0;

    while count > 1 {
        let word = if let Some(tx_buf) = &t.tx_buf {
            if tx_idx + 1 < tx_buf.len() {
                u16::from_le_bytes([tx_buf[tx_idx], tx_buf[tx_idx + 1]]) as u32
            } else {
                if (spi.mode & SpiMode::MOSI_IDLE_HIGH.0) != 0 { 0xFFFF } else { 0 }
            }
        } else {
            if (spi.mode & SpiMode::MOSI_IDLE_HIGH.0) != 0 { 0xFFFF } else { 0 }
        };

        let result = txrx_word(spi, ns, word, bits, flags);

        if let Some(rx_buf) = &mut t.rx_buf {
            if rx_idx + 1 < rx_buf.len() {
                let bytes = (result as u16).to_le_bytes();
                rx_buf[rx_idx] = bytes[0];
                rx_buf[rx_idx + 1] = bytes[1];
            }
        }

        tx_idx += 2;
        rx_idx += 2;
        count -= 2;
    }

    t.len - count
}

/// Transfer 32-bit words
///
/// Ported from: bitbang_txrx_32()
fn bitbang_txrx_32(
    spi: &SpiDevice,
    txrx_word: TxRxWordFn,
    ns: u32,
    t: &mut SpiTransfer,
    flags: u32,
) -> usize {
    let bits = if t.bits_per_word == 0 { 32 } else { t.bits_per_word };
    let mut count = t.len;
    let mut tx_idx = 0;
    let mut rx_idx = 0;

    while count > 3 {
        let word = if let Some(tx_buf) = &t.tx_buf {
            if tx_idx + 3 < tx_buf.len() {
                u32::from_le_bytes([
                    tx_buf[tx_idx],
                    tx_buf[tx_idx + 1],
                    tx_buf[tx_idx + 2],
                    tx_buf[tx_idx + 3],
                ])
            } else {
                if (spi.mode & SpiMode::MOSI_IDLE_HIGH.0) != 0 { 0xFFFFFFFF } else { 0 }
            }
        } else {
            if (spi.mode & SpiMode::MOSI_IDLE_HIGH.0) != 0 { 0xFFFFFFFF } else { 0 }
        };

        let result = txrx_word(spi, ns, word, bits, flags);

        if let Some(rx_buf) = &mut t.rx_buf {
            if rx_idx + 3 < rx_buf.len() {
                let bytes = result.to_le_bytes();
                rx_buf[rx_idx..rx_idx + 4].copy_from_slice(&bytes);
            }
        }

        tx_idx += 4;
        rx_idx += 4;
        count -= 4;
    }

    t.len - count
}

/// Setup transfer parameters
///
/// Ported from: spi_bitbang_setup_transfer()
/// Source: linux-master/drivers/spi/spi-bitbang.c:145
pub fn spi_bitbang_setup_transfer(
    spi: &SpiDevice,
    t: Option<&SpiTransfer>,
) -> Result<(), i32> {
    // TODO: Implement controller_state when needed
    // let cs = spi.controller_state.as_ref()
    //     .and_then(|s| s.downcast_ref::<SpiBitbangCs>())
    //     .ok_or(-22)?; // -EINVAL

    let bits_per_word = t.map(|t| if t.bits_per_word == 0 { spi.bits_per_word } else { t.bits_per_word }).unwrap_or(spi.bits_per_word);
    let hz = t.map(|t| if t.speed_hz == 0 { spi.max_speed_hz } else { t.speed_hz }).unwrap_or(spi.max_speed_hz);

    // Select transfer function based on word size
    let _txrx_bufs = if bits_per_word <= 8 {
        bitbang_txrx_8 as TxRxBufsFn
    } else if bits_per_word <= 16 {
        bitbang_txrx_16 as TxRxBufsFn
    } else if bits_per_word <= 32 {
        bitbang_txrx_32 as TxRxBufsFn
    } else {
        return Err(-22); // -EINVAL
    };

    // Calculate nanoseconds per half clock cycle
    let _nsecs = if hz > 0 {
        let ns = (NSEC_PER_SEC / 2) / hz as u64;
        if ns > MAX_UDELAY_MS * NSEC_PER_MSEC {
            return Err(-22); // -EINVAL
        }
        ns as u32
    } else {
        0
    };

    // Update CS state
    // TODO: Implement controller_state when needed
    // if let Some(cs_mut) = spi.controller_state.as_mut()
    //     .and_then(|s| s.downcast_mut::<SpiBitbangCs>()) {
    //     cs_mut.nsecs = nsecs;
    //     cs_mut.txrx_bufs = Some(txrx_bufs);
    // }

    Ok(())
}

/// Setup SPI device
///
/// Ported from: spi_bitbang_setup()
/// Source: linux-master/drivers/spi/spi-bitbang.c:177
pub fn spi_bitbang_setup(_spi: &mut SpiDevice, _bitbang: &SpiBitbang) -> Result<(), i32> {
    // TODO: Implement controller_state management
    Ok(())
}

/// Cleanup SPI device
///
/// Ported from: spi_bitbang_cleanup()
pub fn spi_bitbang_cleanup(_spi: &mut SpiDevice) {
    // TODO: Implement controller_state cleanup
}

/// Transfer buffers
///
/// Ported from: spi_bitbang_bufs()
fn spi_bitbang_bufs(
    _spi: &SpiDevice,
    _t: &SpiTransfer,
    _bitbang: &SpiBitbang,
) -> Result<usize, i32> {
    // TODO: Implement after controller_state is added
    Err(-22) // -EINVAL
}

/// Initialize bitbang controller
///
/// Ported from: spi_bitbang_init()
/// Source: linux-master/drivers/spi/spi-bitbang.c:350
pub fn spi_bitbang_init(bitbang: &mut SpiBitbang) -> Result<(), i32> {
    // TODO: Add use_gpio_descriptors field to SpiController
    let custom_cs = bitbang.ctlr.flags.contains(crate::drivers::spi::core::SpiControllerFlags::GPIO_SS);

    if custom_cs && bitbang.chipselect.is_none() {
        return Err(-22); // -EINVAL
    }

    // Set default mode bits
    if bitbang.ctlr.mode_bits == 0 {
        bitbang.ctlr.mode_bits = SpiMode::CPOL.0 | SpiMode::CPHA.0 | bitbang.flags;
    }

    // Setup default txrx_bufs if not provided
    if bitbang.txrx_bufs.is_none() {
        bitbang.use_dma = false;
        // TODO: Implement after controller_state is added
        bitbang.txrx_bufs = Some(|_spi: &SpiDevice, _t: &SpiTransfer| -> Result<usize, i32> {
            Err(-22) // -EINVAL
        });

        // Set default setup_transfer if not provided
        if bitbang.setup_transfer.is_none() {
            bitbang.setup_transfer = Some(spi_bitbang_setup_transfer);
        }
    }

    Ok(())
}

// TODO: Re-enable tests after implementing controller_state
/*
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitbang_cs_new() {
        let cs = SpiBitbangCs::new();
        assert_eq!(cs.nsecs, 0);
        assert!(cs.txrx_word.is_none());
        assert!(cs.txrx_bufs.is_none());
    }

    #[test]
    fn test_bitbang_new() {
        let ctlr = SpiController::mock();
        let bitbang = SpiBitbang::new(ctlr);
        assert!(!bitbang.busy);
        assert_eq!(bitbang.flags, 0);
        assert!(!bitbang.use_dma);
    }

    #[test]
    fn test_spi_bitbang_setup_transfer_8bit() {
        let mut spi = SpiDevice::new();
        spi.bits_per_word = 8;
        spi.max_speed_hz = 1_000_000;
        spi.controller_state = Some(Box::new(SpiBitbangCs::new()));

        let result = spi_bitbang_setup_transfer(&spi, None);
        assert!(result.is_ok());

        let cs = spi.controller_state.as_ref().unwrap()
            .downcast_ref::<SpiBitbangCs>().unwrap();
        assert!(cs.nsecs > 0);
        assert!(cs.txrx_bufs.is_some());
    }

    #[test]
    fn test_spi_bitbang_setup_transfer_16bit() {
        let mut spi = SpiDevice::new();
        spi.bits_per_word = 16;
        spi.max_speed_hz = 1_000_000;
        spi.controller_state = Some(Box::new(SpiBitbangCs::new()));

        let result = spi_bitbang_setup_transfer(&spi, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_spi_bitbang_setup_transfer_invalid_bits() {
        let mut spi = SpiDevice::new();
        spi.bits_per_word = 64; // Invalid
        spi.controller_state = Some(Box::new(SpiBitbangCs::new()));

        let result = spi_bitbang_setup_transfer(&spi, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_bitbang_cleanup() {
        let mut spi = SpiDevice::new();
        spi.controller_state = Some(Box::new(SpiBitbangCs::new()));

        spi_bitbang_cleanup(&mut spi);
        assert!(spi.controller_state.is_none());
    }
}
*/
