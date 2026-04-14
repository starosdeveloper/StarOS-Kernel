// SPDX-License-Identifier: MIT
//! TextSurface — a compositor surface that renders static or dynamic text.
//!
//! Instead of drawing text into the back-buffer directly, a `TextSurface`
//! lives in the compositor's surface table just like any other [`Surface`].
//! The compositor calls `render_into()` before compositing, which writes
//! the glyph pixels into the back-buffer at the surface's position.
//!
//! # Example
//! ```ignore
//! let mut ts = TextSurface::new(10, 10, "StarOS v0.1-alpha", WHITE, None);
//! ts.z_order = 10;
//!
//! // Register in the display server and remember the handle.
//! let handle = ds.add_text_surface(ts);
//!
//! // Later, update the label:
//! ds.text_surface_mut(handle).unwrap().set_text("StarOS v0.2");
//! ```

use crate::drivers::display::font::{self, GLYPH_W, GLYPH_H};
use crate::drivers::display::framebuffer::rgb;

/// Maximum number of bytes in a `TextSurface` label (no heap).
pub const MAX_TEXT_LEN: usize = 128;

/// A surface that contains a line of text rendered with the 8×16 bitmap font.
///
/// The text is baked into the compositor's back-buffer each frame by calling
/// [`TextSurface::render_into`].  For dynamic labels (counters, status) update
/// the text with [`set_text`](TextSurface::set_text) and the next `present()`
/// will pick up the change automatically.
pub struct TextSurface {
    // ---- position & visibility ----
    /// Top-left X coordinate on screen.
    pub x: u32,
    /// Top-left Y coordinate on screen.
    pub y: u32,
    pub z_order: u8,
    pub visible: bool,

    // ---- style ----
    /// Foreground (glyph) color, ARGB8888.
    pub fg: u32,
    /// Background color.  `None` = transparent (only glyph pixels written).
    pub bg: Option<u32>,

    // ---- content ----
    buf:  [u8; MAX_TEXT_LEN],
    len:  usize,
}

impl TextSurface {
    /// Create a new text surface.
    ///
    /// `text` is truncated to `MAX_TEXT_LEN` bytes if longer.
    pub fn new(x: u32, y: u32, text: &str, fg: u32, bg: Option<u32>) -> Self {
        let mut ts = Self {
            x, y,
            z_order: 0,
            visible: true,
            fg, bg,
            buf: [0u8; MAX_TEXT_LEN],
            len: 0,
        };
        ts.set_text(text);
        ts
    }

    /// Replace the displayed string.
    pub fn set_text(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let n = bytes.len().min(MAX_TEXT_LEN);
        self.buf[..n].copy_from_slice(&bytes[..n]);
        self.len = n;
    }

    /// Append a single ASCII byte.  Does nothing if the buffer is full.
    pub fn push_byte(&mut self, b: u8) {
        if self.len < MAX_TEXT_LEN {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }

    /// Remove the last byte (backspace).
    pub fn pop_byte(&mut self) {
        if self.len > 0 {
            self.len -= 1;
        }
    }

    /// Clear the text content.
    pub fn clear_text(&mut self) {
        self.len = 0;
    }

    /// Current text as a `&str`.
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    /// Width occupied by the current text, in pixels.
    pub fn pixel_width(&self) -> u32 {
        self.len as u32 * GLYPH_W
    }

    /// Height of one text line, in pixels.
    pub const fn pixel_height() -> u32 {
        GLYPH_H
    }

    /// Render the text into `back_buf` at this surface's `(x, y)` position.
    ///
    /// Called by the compositor (or manually) before `DoubleBuffer::flip()`.
    ///
    /// * `back_buf` — the back-buffer pixel slice.
    /// * `stride`   — pixels per row.
    /// * `height`   — buffer height (for bounds checking).
    pub fn render_into(&self, back_buf: &mut [u32], stride: u32, height: u32) {
        if !self.visible { return; }
        font::draw_str(
            back_buf,
            stride,
            height,
            self.x,
            self.y,
            self.text(),
            self.fg,
            self.bg,
        );
    }
}

// ---------------------------------------------------------------------------
// Common color constants re-exported for convenience
// ---------------------------------------------------------------------------
pub const WHITE:   u32 = rgb(255, 255, 255);
pub const BLACK:   u32 = rgb(0,   0,   0);
pub const YELLOW:  u32 = rgb(255, 255, 0);
pub const GREEN:   u32 = rgb(0,   255, 0);
pub const RED:     u32 = rgb(255, 0,   0);
pub const CYAN:    u32 = rgb(0,   255, 255);
