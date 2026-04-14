// SPDX-License-Identifier: MIT
//! Double-buffering for flicker-free display updates.
//!
//! # How it works
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────────┐
//!  │  CPU renders into  ──►  back_buffer  (ordinary kernel RAM)   │
//!  │                                                              │
//!  │  flip() copies  back_buffer ──► front_buffer (video RAM)     │
//!  │                                                              │
//!  │  Display hardware scans from  front_buffer  continuously     │
//!  └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! The back buffer is allocated as a fixed-size static array so that
//! we never need a heap allocator.  Maximum supported resolution is
//! defined by `MAX_FB_PIXELS`.

use super::framebuffer::Framebuffer;

/// Maximum pixels the static back-buffer can hold.
/// 1920×1080 = 2 073 600 ≈ 2 M pixels.  Adjust for your target device.
pub const MAX_FB_PIXELS: usize = 2_073_600;

/// Double-buffer state.
///
/// The *back buffer* lives in ordinary kernel RAM — the CPU renders into it
/// without touching video memory.  `flip()` then copies the finished frame
/// atomically (from the display's perspective) to the hardware framebuffer.
pub struct DoubleBuffer {
    /// Software back-buffer (kernel RAM).  Always `stride * height` pixels.
    back: &'static mut [u32],
    /// Logical dimensions cached from the hardware framebuffer.
    width: u32,
    height: u32,
    stride: u32,
}

impl DoubleBuffer {
    /// Initialise double-buffering backed by `storage`.
    ///
    /// `storage` must be at least `fb.stride * fb.height` elements long.
    /// Typically you pass a reference to a `static mut` array declared at
    /// the call site so the back buffer has a known, fixed address.
    ///
    /// # Safety
    /// `storage` must remain valid for the lifetime of the kernel and must
    /// not alias any other live reference.
    pub unsafe fn new(
        fb: &Framebuffer,
        storage: &'static mut [u32],
    ) -> Result<Self, BufferError> {
        let needed = (fb.stride * fb.height) as usize;
        if storage.len() < needed {
            return Err(BufferError::StorageTooSmall {
                needed,
                got: storage.len(),
            });
        }

        // Zero the back-buffer so we start with a black screen.
        for px in storage.iter_mut() {
            *px = 0;
        }

        Ok(Self {
            back: storage,
            width: fb.width,
            height: fb.height,
            stride: fb.stride,
        })
    }

    // -----------------------------------------------------------------------
    // Drawing API  (all rendering goes to the back buffer)
    // -----------------------------------------------------------------------

    /// Write a single pixel to the back buffer.
    #[inline(always)]
    pub fn set_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x < self.width && y < self.height {
            let idx = (y * self.stride + x) as usize;
            self.back[idx] = color;
        }
    }

    /// Fill the entire back buffer with `color`.
    pub fn clear(&mut self, color: u32) {
        let len = (self.stride * self.height) as usize;
        for px in &mut self.back[..len] {
            *px = color;
        }
    }

    /// Draw a filled rectangle.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        for row in y..y_end {
            let row_start = (row * self.stride) as usize;
            for col in x..x_end {
                self.back[row_start + col as usize] = color;
            }
        }
    }

    /// Draw a horizontal line.
    #[inline]
    pub fn hline(&mut self, x: u32, y: u32, len: u32, color: u32) {
        self.fill_rect(x, y, len, 1, color);
    }

    /// Draw a vertical line.
    pub fn vline(&mut self, x: u32, y: u32, len: u32, color: u32) {
        if x >= self.width { return; }
        let y_end = (y + len).min(self.height);
        for row in y..y_end {
            self.back[(row * self.stride + x) as usize] = color;
        }
    }

    /// Draw a hollow rectangle (border only, `thickness` pixels wide).
    pub fn draw_rect_border(
        &mut self,
        x: u32, y: u32,
        w: u32, h: u32,
        thickness: u32,
        color: u32,
    ) {
        let t = thickness;
        self.fill_rect(x,         y,         w, t, color); // top
        self.fill_rect(x,         y + h - t, w, t, color); // bottom
        self.fill_rect(x,         y,         t, h, color); // left
        self.fill_rect(x + w - t, y,         t, h, color); // right
    }

    /// Copy `src` scanline-by-scanline into the back buffer.
    ///
    /// `src` stride is assumed equal to `self.stride`.
    pub fn blit_buf(&mut self, src: &[u32]) {
        let len = ((self.stride * self.height) as usize).min(src.len());
        self.back[..len].copy_from_slice(&src[..len]);
    }

    // -----------------------------------------------------------------------
    // Text rendering
    // -----------------------------------------------------------------------

    /// Draw a single ASCII character at `(x, y)`.
    ///
    /// * `fg` — foreground color (ARGB8888).
    /// * `bg` — background fill; `None` = transparent (only fg pixels written).
    pub fn draw_char(&mut self, x: u32, y: u32, ch: u8, fg: u32, bg: Option<u32>) {
        super::font::draw_char(
            &mut self.back,
            self.stride,
            self.height,
            x, y, ch, fg, bg,
        );
    }

    /// Draw a UTF-8 string at `(x, y)`.
    ///
    /// Non-ASCII bytes are rendered as a space.  `\n` wraps to the next line
    /// aligned to the original `x`.
    ///
    /// * `fg` — foreground color.
    /// * `bg` — background; `None` = transparent.
    ///
    /// Returns the `(x, y)` position immediately after the last character.
    pub fn draw_text(
        &mut self,
        x:  u32,
        y:  u32,
        s:  &str,
        fg: u32,
        bg: Option<u32>,
    ) -> (u32, u32) {
        super::font::draw_str(
            &mut self.back,
            self.stride,
            self.height,
            x, y, s, fg, bg,
        )
    }

    /// Convenience: draw white text on transparent background.
    #[inline]
    pub fn print(&mut self, x: u32, y: u32, s: &str, color: u32) -> (u32, u32) {
        self.draw_text(x, y, s, color, None)
    }

    // -----------------------------------------------------------------------
    // Page flip
    // -----------------------------------------------------------------------

    /// **Flip** — copy the finished back buffer into the hardware framebuffer.
    ///
    /// After this call the display shows the newly rendered frame.
    /// The back buffer is left intact so you can do incremental updates.
    ///
    /// # Safety
    /// The caller must ensure no other code is writing to `fb` concurrently.
    pub unsafe fn flip(&self, fb: &mut Framebuffer) {
        let len = (self.stride * self.height) as usize;
        fb.blit_full(&self.back[..len]);
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Dimensions `(width, height)`.
    #[inline]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Read-only view of the back buffer (useful for screenshots / testing).
    #[inline]
    pub fn back_buffer(&self) -> &[u32] {
        let len = (self.stride * self.height) as usize;
        &self.back[..len]
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum BufferError {
    StorageTooSmall { needed: usize, got: usize },
}
