// SPDX-License-Identifier: MIT
//! Display driver subsystem
//!
//! Two layers:
//!
//! 1. [`framebuffer`] — raw HW access (volatile writes to video RAM).
//! 2. [`double_buffer`] — software back-buffer + page-flip mechanism.
//!
//! Typical initialisation sequence:
//!
//! ```ignore
//! // 1. Create the HW framebuffer descriptor (addr from device tree / UEFI).
//! let fb = unsafe {
//!     Framebuffer::new(0x9D40_0000, 1080, 1920, 1080, PixelFormat::Argb8888)
//! };
//!
//! // 2. Back-buffer storage (static, no heap needed).
//! static mut BB_STORAGE: [u32; MAX_FB_PIXELS] = [0u32; MAX_FB_PIXELS];
//!
//! // 3. Wire them together.
//! let db = unsafe { DoubleBuffer::new(&fb, &mut BB_STORAGE).unwrap() };
//!
//! // 4. Hand both off to DisplayServer (in display_server module).
//! ```

pub mod framebuffer;
pub mod double_buffer;
pub mod font;

pub use framebuffer::{
    Framebuffer, PixelFormat,
    rgb, rgba,
    BLACK, WHITE, RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA, GRAY,
};
pub use double_buffer::{DoubleBuffer, BufferError, MAX_FB_PIXELS};
pub use font::{GLYPH_W, GLYPH_H, draw_char, draw_str, text_width};
