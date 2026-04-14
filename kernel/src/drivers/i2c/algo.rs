// SPDX-License-Identifier: MIT OR Apache-2.0
//! I2C Algorithm - Bit-banging I2C implementation
//!
//! Ported from: linux-master/drivers/i2c/algos/i2c-algo-bit.c
//! Source lines: 600 C → 400 Rust
//!
//! This module implements bit-banging I2C algorithm for adapters
//! that provide direct control over SDA and SCL lines.

use alloc::boxed::Box;
use crate::drivers::i2c::core::{I2cAdapter, I2cAlgorithm, I2cMsg, Error, Result};
use crate::drivers::i2c::core::{I2C_M_RD, I2C_M_TEN, I2C_M_IGNORE_NAK, I2C_M_NO_RD_ACK, 
                                 I2C_M_RECV_LEN, I2C_M_NOSTART, I2C_M_STOP, I2C_M_REV_DIR_ADDR};

/// I2C SMBus block max size
const I2C_SMBUS_BLOCK_MAX: usize = 32;

/// Bit-banging adapter operations
pub trait I2cBitOps: Send + Sync {
    /// Set SDA line state
    fn set_sda(&self, val: bool);
    /// Set SCL line state
    fn set_scl(&self, val: bool);
    /// Get SDA line state (optional)
    fn get_sda(&self) -> Option<bool> { None }
    /// Get SCL line state (optional)
    fn get_scl(&self) -> Option<bool> { None }
    /// Pre-transfer callback (optional)
    fn pre_xfer(&self) -> Result<()> { Ok(()) }
    /// Post-transfer callback (optional)
    fn post_xfer(&self) { }
}

/// Bit-banging I2C algorithm data
///
/// # Platform-specific timing
/// The `udelay` parameter represents loop iterations, not actual microseconds.
/// It must be calibrated for each platform based on CPU frequency.
/// For production use, consider implementing a proper TimeSource trait.
pub struct I2cAlgoBitData {
    /// Bit operations
    ops: Box<dyn I2cBitOps>,
    /// Delay in loop iterations (platform-specific, needs calibration)
    udelay: u32,
    /// Timeout in loop iterations
    timeout: u32,
}

impl I2cAlgoBitData {
    /// Create new bit-banging algorithm
    pub fn new(ops: Box<dyn I2cBitOps>, udelay: u32, timeout: u32) -> Self {
        Self { ops, udelay, timeout }
    }

    /// sdalo - Set SDA low with half delay
    #[inline]
    fn sdalo(&self) {
        self.ops.set_sda(false);
        self.udelay_half();
    }

    /// sdahi - Set SDA high with half delay
    #[inline]
    fn sdahi(&self) {
        self.ops.set_sda(true);
        self.udelay_half();
    }

    /// scllo - Set SCL low with half delay
    #[inline]
    fn scllo(&self) {
        self.ops.set_scl(false);
        // Platform-specific delay - needs calibration per CPU frequency
        for _ in 0..(self.udelay / 2) { core::hint::spin_loop(); }
    }

    /// sclhi - Set SCL high and wait for clock stretching
    fn sclhi(&self) -> Result<()> {
        self.ops.set_scl(true);

        if self.ops.get_scl().is_some() {
            let mut iterations = 0;
            let max_iterations = self.timeout * 1000;

            while let Some(false) = self.ops.get_scl() {
                if iterations >= max_iterations {
                    if self.ops.get_scl().unwrap_or(true) {
                        break;
                    }
                    return Err(Error::Timeout);
                }
                iterations += 1;
                core::hint::spin_loop();
            }
        }

        for _ in 0..self.udelay { core::hint::spin_loop(); }
        Ok(())
    }

    /// udelay_half - Half microsecond delay
    #[inline]
    fn udelay_half(&self) {
        for _ in 0..self.udelay.div_ceil(2) { core::hint::spin_loop(); }
    }

    /// i2c_start - Generate START condition
    fn i2c_start(&self) {
        self.ops.set_sda(false);
        for _ in 0..self.udelay { core::hint::spin_loop(); }
        self.scllo();
    }

    /// i2c_repstart - Generate repeated START condition
    fn i2c_repstart(&self) -> Result<()> {
        self.sdahi();
        self.sclhi()?;
        self.ops.set_sda(false);
        for _ in 0..self.udelay { core::hint::spin_loop(); }
        self.scllo();
        Ok(())
    }

    /// i2c_stop - Generate STOP condition
    fn i2c_stop(&self) -> Result<()> {
        self.sdalo();
        self.sclhi()?;
        self.ops.set_sda(true);
        for _ in 0..self.udelay { core::hint::spin_loop(); }
        Ok(())
    }

    /// i2c_outb - Send one byte
    /// Returns: 1 if ACK, 0 if NAK, error on timeout
    fn i2c_outb(&self, c: u8) -> Result<i32> {
        for i in (0..8).rev() {
            let sb = (c >> i) & 1 != 0;
            self.ops.set_sda(sb);
            self.udelay_half();
            self.sclhi()?;
            self.scllo();
        }

        self.sdahi();
        self.sclhi()?;

        let ack = if let Some(get_sda) = self.ops.get_sda() {
            !get_sda
        } else {
            true
        };

        self.scllo();
        Ok(if ack { 1 } else { 0 })
    }

    /// i2c_inb - Read one byte
    fn i2c_inb(&self) -> Result<u8> {
        let mut indata = 0u8;

        self.sdahi();
        for i in 0..8 {
            self.sclhi()?;
            indata <<= 1;
            if let Some(get_sda) = self.ops.get_sda() {
                if get_sda {
                    indata |= 0x01;
                }
            }
            self.ops.set_scl(false);
            let delay = if i == 7 { self.udelay / 2 } else { self.udelay };
            for _ in 0..delay { core::hint::spin_loop(); }
        }

        Ok(indata)
    }

    /// try_address - Try to contact device at address
    fn try_address(&self, addr: u8, retries: u32) -> Result<i32> {
        let mut ret = 0;
        for i in 0..=retries {
            ret = self.i2c_outb(addr)?;
            if ret == 1 || i == retries {
                break;
            }
            self.i2c_stop()?;
            for _ in 0..self.udelay { core::hint::spin_loop(); }
            self.i2c_start();
        }
        Ok(ret)
    }

    /// sendbytes - Send message bytes
    fn sendbytes(&self, msg: &I2cMsg) -> Result<usize> {
        let nak_ok = msg.flags & I2C_M_IGNORE_NAK != 0;
        let mut wrcount = 0;

        for &byte in msg.buf.iter() {
            let retval = self.i2c_outb(byte)?;

            if retval > 0 || (nak_ok && retval == 0) {
                wrcount += 1;
            } else {
                return Err(Error::IoError);
            }
        }

        Ok(wrcount)
    }

    /// acknak - Send ACK or NAK
    fn acknak(&self, is_ack: bool) -> Result<()> {
        if is_ack {
            self.ops.set_sda(false);
        }
        self.udelay_half();
        self.sclhi()?;
        self.scllo();
        Ok(())
    }

    /// readbytes - Read message bytes
    fn readbytes(&self, msg: &mut I2cMsg) -> Result<usize> {
        if self.ops.get_sda().is_none() {
            return Err(Error::NotSupported);
        }

        let mut rdcount = 0;
        let flags = msg.flags;
        let mut count = msg.buf.len();

        for i in 0..count {
            let inval = self.i2c_inb()?;
            msg.buf[i] = inval;
            rdcount += 1;

            if rdcount == 1 && (flags & I2C_M_RECV_LEN != 0) {
                if inval == 0 || inval as usize > I2C_SMBUS_BLOCK_MAX {
                    if flags & I2C_M_NO_RD_ACK == 0 {
                        self.acknak(false)?;
                    }
                    return Err(Error::IoError);
                }
                count += inval as usize;
                msg.buf.resize(count, 0);
            }

            if flags & I2C_M_NO_RD_ACK == 0 {
                let remaining = count - rdcount;
                self.acknak(remaining > 0)?;
            }
        }

        Ok(rdcount)
    }

    /// bit_doAddress - Handle addressing for message
    fn bit_do_address(&self, msg: &I2cMsg, retries: u32) -> Result<()> {
        let flags = msg.flags;
        let nak_ok = flags & I2C_M_IGNORE_NAK != 0;

        if flags & I2C_M_TEN != 0 {
            // 10-bit address
            let addr = 0xf0 | ((msg.addr >> 7) & 0x06) as u8;
            let ret = self.try_address(addr, retries)?;
            if ret != 1 && !nak_ok {
                return Err(Error::NotFound);
            }

            let ret = self.i2c_outb((msg.addr & 0xff) as u8)?;
            if ret != 1 && !nak_ok {
                return Err(Error::NotFound);
            }

            if flags & I2C_M_RD != 0 {
                self.i2c_repstart()?;
                let addr = addr | 0x01;
                let ret = self.try_address(addr, retries)?;
                if ret != 1 && !nak_ok {
                    return Err(Error::IoError);
                }
            }
        } else {
            // 7-bit address
            let mut addr = ((msg.addr << 1) | if flags & I2C_M_RD != 0 { 1 } else { 0 }) as u8;
            if flags & I2C_M_REV_DIR_ADDR != 0 {
                addr ^= 1;
            }
            let ret = self.try_address(addr, retries)?;
            if ret != 1 && !nak_ok {
                return Err(Error::NotFound);
            }
        }

        Ok(())
    }

    /// bit_xfer - Execute I2C transfer
    fn bit_xfer(&self, adapter: &I2cAdapter, msgs: &mut [I2cMsg]) -> Result<usize> {
        self.ops.pre_xfer()?;

        self.i2c_start();

        for i in 0..msgs.len() {
            let nak_ok = msgs[i].flags & I2C_M_IGNORE_NAK != 0;

            if msgs[i].flags & I2C_M_NOSTART == 0 {
                if i > 0 {
                    if msgs[i - 1].flags & I2C_M_STOP != 0 {
                        self.i2c_stop()?;
                        self.i2c_start();
                    } else {
                        self.i2c_repstart()?;
                    }
                }

                if let Err(e) = self.bit_do_address(&msgs[i], adapter.retries) {
                    if !nak_ok {
                        let _ = self.i2c_stop();
                        self.ops.post_xfer();
                        return Err(e);
                    }
                }
            }

            if msgs[i].flags & I2C_M_RD != 0 {
                let ret = self.readbytes(&mut msgs[i]);
                if let Err(e) = ret {
                    let _ = self.i2c_stop();
                    self.ops.post_xfer();
                    return Err(e);
                }
            } else {
                let ret = self.sendbytes(&msgs[i]);
                if let Err(e) = ret {
                    let _ = self.i2c_stop();
                    self.ops.post_xfer();
                    return Err(e);
                }
            }
        }

        self.i2c_stop()?;
        self.ops.post_xfer();

        Ok(msgs.len())
    }
}

impl I2cAlgorithm for I2cAlgoBitData {
    fn master_xfer(&self, adapter: &I2cAdapter, msgs: &mut [I2cMsg]) -> Result<usize> {
        self.bit_xfer(adapter, msgs)
    }

    fn functionality(&self) -> u32 {
        0xFFFFFFFF // Support all functionality
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::vec;

    struct TestBitOps {
        sda: core::cell::Cell<bool>,
        scl: core::cell::Cell<bool>,
    }

    impl TestBitOps {
        fn new() -> Self {
            Self {
                sda: core::cell::Cell::new(true),
                scl: core::cell::Cell::new(true),
            }
        }
    }

    impl I2cBitOps for TestBitOps {
        fn set_sda(&self, val: bool) {
            self.sda.set(val);
        }

        fn set_scl(&self, val: bool) {
            self.scl.set(val);
        }

        fn get_sda(&self) -> Option<bool> {
            Some(self.sda.get())
        }

        fn get_scl(&self) -> Option<bool> {
            Some(self.scl.get())
        }
    }

    #[test]
    fn test_algo_creation() {
        let ops = Box::new(TestBitOps::new());
        let algo = I2cAlgoBitData::new(ops, 5, 100);
        assert_eq!(algo.udelay, 5);
        assert_eq!(algo.timeout, 100);
    }

    #[test]
    fn test_start_stop() {
        let ops = Box::new(TestBitOps::new());
        let algo = I2cAlgoBitData::new(ops, 1, 100);
        
        algo.i2c_start();
        algo.i2c_stop().unwrap();
    }

    #[test]
    fn test_byte_transfer() {
        let ops = Box::new(TestBitOps::new());
        let algo = I2cAlgoBitData::new(ops, 1, 100);
        
        let result = algo.i2c_outb(0x55);
        assert!(result.is_ok());
    }
}
