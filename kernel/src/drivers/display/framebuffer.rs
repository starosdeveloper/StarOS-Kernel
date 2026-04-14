// SPDX-License-Identifier: MIT
//! Raw framebuffer hardware driver
//!
//! Direct bare-metal access to the physical framebuffer memory.
//! No external dependencies — pure no_std Rust.

use core::ptr;

/// Pixel format supported by the framebuffer
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PixelFormat {
    /// 0xAARRGGBB
    Argb8888,
    /// 0xRRGGBBAA
    Rgba8888,
    /// 0x00RRGGBB (upper byte ignored)
    Rgb888,
}

/// Hardware framebuffer descriptor
///
/// Holds the physical address and geometry of the display memory.
/// All writes go directly to the hardware via `write_volatile`.
pub struct Framebuffer {
    /// Physical base address mapped into kernel virtual space
    base: *mut u32,
    pub width: u32,
    pub height: u32,
    /// Pixels per row (may differ from width due to alignment padding)
    pub stride: u32,
    pub format: PixelFormat,
}

// SAFETY: The framebuffer is a unique, globally-owned hardware resource.
// Only one owner writes to it at a time (enforced by DisplayServer).
unsafe impl Send for Framebuffer {}
unsafe impl Sync for Framebuffer {}

impl Framebuffer {
    /// Construct a framebuffer descriptor.
    ///
    /// # Safety
    /// `addr` must be a valid, mapped physical framebuffer address that
    /// remains valid for the lifetime of the kernel.
    pub const unsafe fn new(
        addr: usize,
        width: u32,
        height: u32,
        stride: u32,
        format: PixelFormat,
    ) -> Self {
        Self {
            base: addr as *mut u32,
            width,
            height,
            stride,
            format,
        }
    }

    /// Write a single pixel directly to hardware memory.
    ///
    /// Out-of-bounds coordinates are silently ignored.
    #[inline(always)]
    pub fn write_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.width && y < self.height {
            // SAFETY: bounds checked above; address arithmetic stays within
            // the allocated framebuffer region.
            unsafe {
                ptr::write_volatile(
                    self.base.add((y * self.stride + x) as usize),
                    color,
                );
            }
        }
    }

    /// Copy an entire buffer slice into the framebuffer in one shot.
    ///
    /// `src` must be exactly `stride * height` elements long.
    /// This is the fast-path used by the double-buffer flip.
    ///
    /// # Safety
    /// `src` slice must be valid for the duration of this call.
    pub unsafe fn blit_full(&mut self, src: &[u32]) {
        let total = (self.stride * self.height) as usize;
        debug_assert_eq!(src.len(), total, "blit_full: src length mismatch");

        let len = src.len().min(total);
        for i in 0..len {
            ptr::write_volatile(self.base.add(i), src[i]);
        }
    }

    /// Fill the entire framebuffer with a solid color.
    pub fn clear(&mut self, color: u32) {
        let total = (self.stride * self.height) as usize;
        for i in 0..total {
            // SAFETY: index is bounded by the framebuffer size.
            unsafe {
                ptr::write_volatile(self.base.add(i), color);
            }
        }
    }

    /// Returns `(width, height)` in pixels.
    #[inline]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Total size of the framebuffer in bytes.
    #[inline]
    pub fn size_bytes(&self) -> usize {
        (self.stride * self.height) as usize * core::mem::size_of::<u32>()
    }
}

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

/// Pack RGB components into ARGB8888 (alpha = 0xFF).
#[inline]
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Pack RGBA components into ARGB8888.
#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub const BLACK:   u32 = rgb(0,   0,   0);
pub const WHITE:   u32 = rgb(255, 255, 255);
pub const RED:     u32 = rgb(255, 0,   0);
pub const GREEN:   u32 = rgb(0,   255, 0);
pub const BLUE:    u32 = rgb(0,   0,   255);
pub const YELLOW:  u32 = rgb(255, 255, 0);
pub const CYAN:    u32 = rgb(0,   255, 255);
pub const MAGENTA: u32 = rgb(255, 0,   255);
pub const GRAY:    u32 = rgb(128, 128, 128);
