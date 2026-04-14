// SPDX-License-Identifier: MIT
//! V4L2 (Video4Linux2) Core
//!
//! Ported from Linux: `drivers/media/v4l2-core/v4l2-dev.c`,
//!                    `v4l2-ioctl.c`, `v4l2-buf.c`, `videobuf2-core.c`
//!
//! Implements:
//! - V4L2 device registration
//! - Pixel format negotiation (VIDIOC_S_FMT / VIDIOC_G_FMT)
//! - Buffer queue management (VIDIOC_REQBUFS / QBUF / DQBUF)
//! - Stream on/off (VIDIOC_STREAMON / STREAMOFF)
//!
//! Buffer lifecycle:
//!   REQBUFS → alloc n buffers in DEQUEUED state
//!   QBUF    → driver takes ownership (→ IN_QUEUE)
//!   hardware fills frame (→ DONE)
//!   DQBUF   → userspace takes ownership (→ DEQUEUED)

use spin::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::error::KernelError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MAX_V4L2_DEVS:    usize = 16;
pub const MAX_V4L2_BUFS:    usize = 8;
pub const V4L2_NAME_LEN:    usize = 32;

// ---------------------------------------------------------------------------
// Pixel formats (fourcc-style identifiers)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum V4l2PixFmt {
    Yuyv    = 0x5659_5559, // V4L2_PIX_FMT_YUYV
    Yvyu    = 0x5559_5659,
    Nv12    = 0x3231_564E, // V4L2_PIX_FMT_NV12 (Y + interleaved UV)
    Nv21    = 0x3132_564E,
    Yuv420  = 0x3032_3449, // V4L2_PIX_FMT_YUV420
    Rgb565  = 0x5042_4752,
    Rgb24   = 0x4742_5233,
    Bgr24   = 0x5233_4742,
    Argb32  = 0x4241_5247,
    Jpeg    = 0x4745_504A, // V4L2_PIX_FMT_JPEG
    H264    = 0x3436_3248,
    Hevc    = 0x4356_4548,
    Raw10   = 0x3031_4752, // V4L2_PIX_FMT_SGRBG10
    Raw12   = 0x3231_4752,
}

impl V4l2PixFmt {
    pub fn bpp(&self) -> u32 {
        match self {
            V4l2PixFmt::Yuyv | V4l2PixFmt::Yvyu => 2,
            V4l2PixFmt::Nv12 | V4l2PixFmt::Nv21 => 1, // 1.5 bpp, approx
            V4l2PixFmt::Yuv420               => 1,
            V4l2PixFmt::Rgb565               => 2,
            V4l2PixFmt::Rgb24 | V4l2PixFmt::Bgr24 => 3,
            V4l2PixFmt::Argb32               => 4,
            V4l2PixFmt::Raw10                => 2, // packed 10-bit
            V4l2PixFmt::Raw12                => 2,
            _                                => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Format descriptor
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct V4l2Fmt {
    pub width:      u32,
    pub height:     u32,
    pub pixfmt:     V4l2PixFmt,
    pub bytesperline: u32,
    pub sizeimage:  u32,
    pub field:      u8,  // 0=none, 1=top, 2=bottom, 3=interlaced
}

impl V4l2Fmt {
    pub fn new(w: u32, h: u32, fmt: V4l2PixFmt) -> Self {
        let bpl = w * fmt.bpp();
        Self {
            width: w, height: h, pixfmt: fmt,
            bytesperline: bpl,
            sizeimage: bpl * h,
            field: 0,
        }
    }
    pub const fn zero() -> Self {
        Self { width: 0, height: 0, pixfmt: V4l2PixFmt::Yuyv,
               bytesperline: 0, sizeimage: 0, field: 0 }
    }
}

// ---------------------------------------------------------------------------
// Buffer management
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufState {
    Dequeued,   // Owned by userspace
    InQueue,    // Handed to driver
    Active,     // Being filled by HW
    Done,       // Frame ready, waiting for DQBUF
    Error,
}

#[derive(Clone, Copy)]
pub struct V4l2Buffer {
    pub index:    u32,
    pub state:    BufState,
    pub phys_addr: u64,  // physical address of pixel data
    pub length:   u32,
    pub bytesused: u32,
    pub sequence: u32,
    pub timestamp_us: u64,
}

impl V4l2Buffer {
    pub const fn empty() -> Self {
        Self { index: 0, state: BufState::Dequeued, phys_addr: 0,
               length: 0, bytesused: 0, sequence: 0, timestamp_us: 0 }
    }
}

// ---------------------------------------------------------------------------
// V4l2Ops vtable
// ---------------------------------------------------------------------------

pub struct V4l2Ops {
    pub open:       fn(hw_idx: u8) -> Result<(), KernelError>,
    pub close:      fn(hw_idx: u8),
    pub set_fmt:    fn(hw_idx: u8, fmt: &V4l2Fmt) -> Result<V4l2Fmt, KernelError>,
    pub stream_on:  fn(hw_idx: u8) -> Result<(), KernelError>,
    pub stream_off: fn(hw_idx: u8),
    pub queue_buf:  fn(hw_idx: u8, buf_idx: u32) -> Result<(), KernelError>,
    pub get_frame:  fn(hw_idx: u8) -> Option<(u32, u32, u64)>, // (buf_idx, bytesused, ts)
}

// ---------------------------------------------------------------------------
// V4L2 device state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V4l2State {
    Closed,
    Opened,
    Configured,
    Streaming,
    Error,
}

pub struct V4l2Dev {
    pub hw_idx:    u8,
    pub ops:       &'static V4l2Ops,
    pub state:     V4l2State,
    pub fmt:       V4l2Fmt,
    pub bufs:      [V4l2Buffer; MAX_V4L2_BUFS],
    pub buf_count: u32,
    pub seq:       AtomicU32,
    pub name:      [u8; V4L2_NAME_LEN],
    pub name_len:  u8,
}

impl V4l2Dev {
    pub const fn new(hw_idx: u8, ops: &'static V4l2Ops, name: &'static [u8]) -> Self {
        let mut n = [0u8; V4L2_NAME_LEN];
        let len = if name.len() < V4L2_NAME_LEN { name.len() } else { V4L2_NAME_LEN };
        // const loops require explicit unrolling
        let mut i = 0;
        while i < len { n[i] = name[i]; i += 1; }
        Self {
            hw_idx, ops,
            state: V4l2State::Closed,
            fmt: V4l2Fmt::zero(),
            bufs: [V4l2Buffer { index: 0, state: BufState::Dequeued, phys_addr: 0,
                                length: 0, bytesused: 0, sequence: 0, timestamp_us: 0 }; MAX_V4L2_BUFS],
            buf_count: 0,
            seq: AtomicU32::new(0),
            name: n,
            name_len: len as u8,
        }
    }

    pub fn name_str(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

// ---------------------------------------------------------------------------
// Global registry
// ---------------------------------------------------------------------------

pub struct V4l2DevTable {
    pub devs:  [Option<V4l2Dev>; MAX_V4L2_DEVS],
    pub count: usize,
}

impl V4l2DevTable {
    pub const fn new() -> Self {
        Self {
            devs: [None, None, None, None, None, None, None, None,
                   None, None, None, None, None, None, None, None],
            count: 0,
        }
    }
}

pub static V4L2_DEVS: Mutex<V4l2DevTable> = Mutex::new(V4l2DevTable::new());

// ---------------------------------------------------------------------------
// API functions
// ---------------------------------------------------------------------------

/// Register a V4L2 device.
pub fn v4l2_register(dev: V4l2Dev) -> Result<u8, KernelError> {
    let mut tbl = V4L2_DEVS.lock();
    if tbl.count >= MAX_V4L2_DEVS {
        return Err(KernelError::ResourceExhausted);
    }
    let slot = tbl.count;
    tbl.devs[slot] = Some(dev);
    tbl.count += 1;
    Ok(slot as u8)
}

/// VIDIOC_REQBUFS: allocate n capture buffers.
pub fn v4l2_reqbufs(slot: u8, count: u32, base_phys: u64) -> Result<u32, KernelError> {
    let actual = count.min(MAX_V4L2_BUFS as u32);
    let mut tbl = V4L2_DEVS.lock();
    let d = tbl.devs[slot as usize].as_mut().ok_or(KernelError::NotFound)?;
    let buf_size = d.fmt.sizeimage;
    for i in 0..actual as usize {
        d.bufs[i] = V4l2Buffer {
            index: i as u32,
            state: BufState::Dequeued,
            phys_addr: base_phys + (i as u64) * (buf_size as u64),
            length: buf_size,
            bytesused: 0,
            sequence: 0,
            timestamp_us: 0,
        };
    }
    d.buf_count = actual;
    d.state = V4l2State::Configured;
    Ok(actual)
}

/// VIDIOC_QBUF: enqueue a buffer for the driver to fill.
pub fn v4l2_qbuf(slot: u8, buf_idx: u32) -> Result<(), KernelError> {
    let (hw_idx, ops) = {
        let mut tbl = V4L2_DEVS.lock();
        let d = tbl.devs[slot as usize].as_mut().ok_or(KernelError::NotFound)?;
        if buf_idx >= d.buf_count {
            return Err(KernelError::InvalidParameter("buf_idx"));
        }
        if d.bufs[buf_idx as usize].state != BufState::Dequeued {
            return Err(KernelError::OperationFailed);
        }
        d.bufs[buf_idx as usize].state = BufState::InQueue;
        (d.hw_idx, d.ops)
    };
    (ops.queue_buf)(hw_idx, buf_idx)?;
    Ok(())
}

/// VIDIOC_DQBUF: dequeue a filled buffer.
pub fn v4l2_dqbuf(slot: u8) -> Result<V4l2Buffer, KernelError> {
    let mut tbl = V4L2_DEVS.lock();
    let d = tbl.devs[slot as usize].as_mut().ok_or(KernelError::NotFound)?;
    for i in 0..d.buf_count as usize {
        if d.bufs[i].state == BufState::Done {
            let buf = d.bufs[i];
            d.bufs[i].state = BufState::Dequeued;
            return Ok(buf);
        }
    }
    Err(KernelError::NotFound) // no done buffer
}

/// VIDIOC_STREAMON: start capture.
pub fn v4l2_stream_on(slot: u8) -> Result<(), KernelError> {
    let (hw_idx, ops) = {
        let mut tbl = V4L2_DEVS.lock();
        let d = tbl.devs[slot as usize].as_mut().ok_or(KernelError::NotFound)?;
        d.state = V4l2State::Streaming;
        (d.hw_idx, d.ops)
    };
    (ops.stream_on)(hw_idx)
}

/// Frame completion IRQ callback: mark a buffer as done.
pub fn v4l2_buf_done(slot: u8, buf_idx: u32, bytesused: u32, ts_us: u64) {
    let mut tbl = V4L2_DEVS.lock();
    if let Some(d) = &mut tbl.devs[slot as usize] {
        if buf_idx < d.buf_count {
            let seq = d.seq.fetch_add(1, Ordering::Relaxed);
            d.bufs[buf_idx as usize].state = BufState::Done;
            d.bufs[buf_idx as usize].bytesused = bytesused;
            d.bufs[buf_idx as usize].sequence = seq;
            d.bufs[buf_idx as usize].timestamp_us = ts_us;
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    static DUMMY_OPS: V4l2Ops = V4l2Ops {
        open:       |_| Ok(()),
        close:      |_| {},
        set_fmt:    |_, fmt| Ok(*fmt),
        stream_on:  |_| Ok(()),
        stream_off: |_| {},
        queue_buf:  |_, _| Ok(()),
        get_frame:  |_| None,
    };

    fn make_dev() -> V4l2Dev {
        V4l2Dev::new(0, &DUMMY_OPS, b"test-camera")
    }

    #[test]
    fn test_v4l2_fmt_bpp() {
        assert_eq!(V4l2PixFmt::Yuyv.bpp(), 2);
        assert_eq!(V4l2PixFmt::Argb32.bpp(), 4);
        assert_eq!(V4l2PixFmt::Rgb24.bpp(), 3);
    }

    #[test]
    fn test_v4l2_fmt_sizeimage() {
        let fmt = V4l2Fmt::new(1920, 1080, V4l2PixFmt::Yuyv);
        assert_eq!(fmt.bytesperline, 1920 * 2);
        assert_eq!(fmt.sizeimage, 1920 * 2 * 1080);
    }

    #[test]
    fn test_v4l2_dev_name() {
        let dev = make_dev();
        assert_eq!(dev.name_str(), b"test-camera");
    }

    #[test]
    fn test_v4l2_register() {
        let mut dev = make_dev();
        dev.fmt = V4l2Fmt::new(1920, 1080, V4l2PixFmt::Nv12);
        let slot = v4l2_register(dev).unwrap();
        assert!(slot < MAX_V4L2_DEVS as u8);
    }

    #[test]
    fn test_v4l2_reqbufs() {
        // Create a fresh device slot
        let mut dev = V4l2Dev::new(10, &DUMMY_OPS, b"cam-reqbufs");
        dev.fmt = V4l2Fmt::new(640, 480, V4l2PixFmt::Yuyv);
        let slot = v4l2_register(dev).unwrap();

        let n = v4l2_reqbufs(slot, 4, 0x9000_0000).unwrap();
        assert_eq!(n, 4);
        let tbl = V4L2_DEVS.lock();
        let d = tbl.devs[slot as usize].as_ref().unwrap();
        assert_eq!(d.buf_count, 4);
        assert_eq!(d.bufs[0].state, BufState::Dequeued);
        assert_eq!(d.bufs[0].phys_addr, 0x9000_0000);
        let stride = 640u64 * 2 * 480;
        assert_eq!(d.bufs[1].phys_addr, 0x9000_0000 + stride);
    }

    #[test]
    fn test_v4l2_qbuf_dqbuf() {
        let mut dev = V4l2Dev::new(11, &DUMMY_OPS, b"cam-qbuf");
        dev.fmt = V4l2Fmt::new(640, 480, V4l2PixFmt::Yuyv);
        let slot = v4l2_register(dev).unwrap();
        v4l2_reqbufs(slot, 2, 0xA000_0000).unwrap();

        assert!(v4l2_qbuf(slot, 0).is_ok());
        // Simulate frame completion
        v4l2_buf_done(slot, 0, 640 * 2 * 480, 1_000_000);
        let buf = v4l2_dqbuf(slot).unwrap();
        assert_eq!(buf.index, 0);
        assert_eq!(buf.sequence, 0);
        assert!(buf.bytesused > 0);
    }

    #[test]
    fn test_v4l2_dqbuf_empty_returns_not_found() {
        let mut dev = V4l2Dev::new(12, &DUMMY_OPS, b"cam-empty");
        dev.fmt = V4l2Fmt::new(640, 480, V4l2PixFmt::Yuyv);
        let slot = v4l2_register(dev).unwrap();
        v4l2_reqbufs(slot, 2, 0xB000_0000).unwrap();
        // No frame done yet
        assert!(v4l2_dqbuf(slot).is_err());
    }

    #[test]
    fn test_v4l2_stream_on() {
        let mut dev = V4l2Dev::new(13, &DUMMY_OPS, b"cam-stream");
        dev.fmt = V4l2Fmt::new(1280, 720, V4l2PixFmt::Nv12);
        let slot = v4l2_register(dev).unwrap();
        v4l2_reqbufs(slot, 3, 0xC000_0000).unwrap();
        assert!(v4l2_stream_on(slot).is_ok());
        let tbl = V4L2_DEVS.lock();
        assert_eq!(tbl.devs[slot as usize].as_ref().unwrap().state, V4l2State::Streaming);
    }

    #[test]
    fn test_v4l2_buf_sequence_increments() {
        let mut dev = V4l2Dev::new(14, &DUMMY_OPS, b"cam-seq");
        dev.fmt = V4l2Fmt::new(640, 480, V4l2PixFmt::Yuyv);
        let slot = v4l2_register(dev).unwrap();
        v4l2_reqbufs(slot, 2, 0xD000_0000).unwrap();

        v4l2_qbuf(slot, 0).unwrap();
        v4l2_buf_done(slot, 0, 100, 1000);
        let b0 = v4l2_dqbuf(slot).unwrap();

        v4l2_qbuf(slot, 0).unwrap();
        v4l2_buf_done(slot, 0, 100, 2000);
        let b1 = v4l2_dqbuf(slot).unwrap();

        assert_eq!(b1.sequence, b0.sequence + 1);
    }
}
