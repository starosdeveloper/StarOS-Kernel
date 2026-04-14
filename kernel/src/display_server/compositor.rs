// SPDX-License-Identifier: MIT
//! Basic compositor — output abstraction layer.
//!
//! ```text
//!   ┌─────────────────────────────────────────────────────┐
//!   │  Compositor                                         │
//!   │  ┌──────────────┐  ┌──────────────┐               │
//!   │  │ Surface 0    │  │ Surface 1    │  …            │
//!   │  │ (solid color)│  │ (pixel buf)  │               │
//!   │  └──────────────┘  └──────────────┘               │
//!   │              compose() ──► DoubleBuffer             │
//!   └─────────────────────────────────────────────────────┘
//! ```
//!
//! # Surface content
//!
//! Each [`Surface`] carries either:
//! - a flat **solid color** (`pixels` is `None`), or
//! - a **pixel buffer** (`pixels` is `Some(SurfacePixels)`) — used for
//!   icons, images, and application window bitmaps.
//!
//! The pixel buffer is referenced by a raw pointer.  The caller is
//! responsible for keeping the underlying data alive as long as the
//! surface is registered with the compositor (standard kernel lifetime
//! discipline — same as framebuffer addresses).

use crate::drivers::display::framebuffer::{rgb, BLACK};

/// Porter-Duff "over" blend, integer arithmetic (no floats).
///
/// `src_*` are pre-extracted ARGB components (0–255 each).
/// `dst` is the packed ARGB8888 destination pixel.
#[inline(always)]
fn alpha_blend(src_r: u32, src_g: u32, src_b: u32, src_a: u32, dst: u32) -> u32 {
    let dst_r = (dst >> 16) & 0xFF;
    let dst_g = (dst >>  8) & 0xFF;
    let dst_b =  dst        & 0xFF;
    let inv_a = 255 - src_a;
    let out_r = (src_r * src_a + dst_r * inv_a) / 255;
    let out_g = (src_g * src_a + dst_g * inv_a) / 255;
    let out_b = (src_b * src_a + dst_b * inv_a) / 255;
    rgb(out_r as u8, out_g as u8, out_b as u8)
}

/// Maximum number of surfaces the compositor tracks simultaneously.
pub const MAX_SURFACES: usize = 32;

/// Blend mode for a surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlendMode {
    /// Overwrite destination pixels completely (fastest).
    /// For pixel-buffer surfaces: source pixel is written as-is.
    Opaque,
    /// Alpha-blend.
    /// For solid-color surfaces: uses `Surface::color`'s alpha byte.
    /// For pixel-buffer surfaces: uses each source pixel's own alpha channel.
    Alpha,
}

/// Reference to a caller-owned ARGB8888 pixel buffer.
///
/// # Safety contract
/// The data pointed to by `ptr` must remain valid and immutable for as long
/// as this `SurfacePixels` value is live inside the compositor.
#[derive(Clone, Copy, Debug)]
pub struct SurfacePixels {
    /// Base pointer to the first pixel of the image (top-left corner).
    pub ptr: *const u32,
    /// Pixels per row in the source buffer.  May be larger than
    /// `Surface::width` when the surface shows a sub-region of a bigger image.
    pub stride: u32,
}

// SAFETY: pixel data is read-only and kernel-owned (static or device memory).
unsafe impl Send for SurfacePixels {}
unsafe impl Sync for SurfacePixels {}

/// A rectangular region drawn by the compositor.
///
/// When `pixels` is `None` the surface is filled with `color`.
/// When `pixels` is `Some(...)` the pixel buffer is blitted/blended instead.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Flat fill color (ARGB8888).  Ignored when `pixels` is `Some`.
    pub color: u32,
    /// Optional pixel buffer.  When `Some`, overrides `color`.
    pub pixels: Option<SurfacePixels>,
    pub blend: BlendMode,
    pub z_order: u8,
    pub visible: bool,
}

impl Surface {
    /// Create a solid-color surface.
    pub const fn new(x: u32, y: u32, width: u32, height: u32, color: u32) -> Self {
        Self {
            x, y, width, height,
            color,
            pixels: None,
            blend: BlendMode::Opaque,
            z_order: 0,
            visible: true,
        }
    }

    /// Create a pixel-buffer surface (icon, image, window bitmap).
    ///
    /// # Safety
    /// `data` must point to at least `src_stride * height` valid `u32` pixels
    /// in ARGB8888 format.  The data must outlive this surface's registration
    /// with the compositor.
    pub unsafe fn with_pixels(
        x: u32, y: u32,
        width: u32, height: u32,
        data: *const u32,
        src_stride: u32,
    ) -> Self {
        Self {
            x, y, width, height,
            color: 0,
            pixels: Some(SurfacePixels { ptr: data, stride: src_stride }),
            blend: BlendMode::Opaque,
            z_order: 0,
            visible: true,
        }
    }

    /// Attach a pixel buffer to an existing solid-color surface.
    ///
    /// # Safety
    /// Same contract as [`with_pixels`](Surface::with_pixels).
    pub unsafe fn set_pixels(&mut self, data: *const u32, src_stride: u32) {
        self.pixels = Some(SurfacePixels { ptr: data, stride: src_stride });
    }

    /// Detach the pixel buffer — surface reverts to solid `color` fill.
    pub fn clear_pixels(&mut self) {
        self.pixels = None;
    }
}

/// The compositor manages a fixed set of surfaces and renders them in
/// z-order into the double-buffer on each `compose()` call.
pub struct Compositor {
    surfaces: [Option<Surface>; MAX_SURFACES],
    /// Screen dimensions — must match the double-buffer.
    width: u32,
    height: u32,
    /// Background fill color (painted before all surfaces).
    bg_color: u32,
}

impl Compositor {
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            surfaces: [None; MAX_SURFACES],
            width,
            height,
            bg_color: BLACK,
        }
    }

    pub fn set_background(&mut self, color: u32) {
        self.bg_color = color;
    }

    // -----------------------------------------------------------------------
    // Surface management
    // -----------------------------------------------------------------------

    /// Add a surface and return its handle (index).
    /// Returns `None` if the surface table is full.
    pub fn add_surface(&mut self, surface: Surface) -> Option<usize> {
        for (i, slot) in self.surfaces.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(surface);
                return Some(i);
            }
        }
        None
    }

    /// Remove a surface by handle.
    pub fn remove_surface(&mut self, handle: usize) {
        if handle < MAX_SURFACES {
            self.surfaces[handle] = None;
        }
    }

    /// Mutably access a surface by handle.
    pub fn surface_mut(&mut self, handle: usize) -> Option<&mut Surface> {
        self.surfaces.get_mut(handle)?.as_mut()
    }

    /// Read-only access to a surface by handle (for hit-testing).
    pub fn surface_ref(&self, handle: usize) -> Option<&Surface> {
        self.surfaces.get(handle)?.as_ref()
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Render all visible surfaces into `back_buf` (back-buffer pixel slice).
    ///
    /// The caller is responsible for calling `DoubleBuffer::flip()` afterwards.
    ///
    /// `stride` is the number of pixels per row in `back_buf`.
    pub fn compose(&self, back_buf: &mut [u32], stride: u32) {
        // 1. Clear to background color.
        let total = (stride * self.height) as usize;
        for px in &mut back_buf[..total] {
            *px = self.bg_color;
        }

        // 2. Collect visible surfaces and sort by z_order (stable, lowest first).
        let mut order: [usize; MAX_SURFACES] = [0; MAX_SURFACES];
        let mut count = 0usize;
        for (i, slot) in self.surfaces.iter().enumerate() {
            if let Some(s) = slot {
                if s.visible {
                    order[count] = i;
                    count += 1;
                }
            }
        }
        // Insertion sort — small N, no alloc needed.
        for i in 1..count {
            let mut j = i;
            while j > 0 {
                let a = self.surfaces[order[j - 1]].unwrap().z_order;
                let b = self.surfaces[order[j]].unwrap().z_order;
                if a > b {
                    order.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }

        // 3. Paint each surface.
        for &idx in &order[..count] {
            if let Some(surf) = &self.surfaces[idx] {
                self.paint_surface(back_buf, stride, surf);
            }
        }
    }

    fn paint_surface(&self, buf: &mut [u32], stride: u32, s: &Surface) {
        match s.pixels {
            None       => self.paint_solid(buf, stride, s),
            Some(pix)  => self.paint_pixels(buf, stride, s, pix),
        }
    }

    // ---- solid color fill --------------------------------------------------

    fn paint_solid(&self, buf: &mut [u32], stride: u32, s: &Surface) {
        let x_end = (s.x + s.width).min(self.width);
        let y_end = (s.y + s.height).min(self.height);

        match s.blend {
            BlendMode::Opaque => {
                for row in s.y..y_end {
                    let row_base = (row * stride) as usize;
                    for col in s.x..x_end {
                        buf[row_base + col as usize] = s.color;
                    }
                }
            }
            BlendMode::Alpha => {
                let src_a = ((s.color >> 24) & 0xFF) as u32;
                let src_r = ((s.color >> 16) & 0xFF) as u32;
                let src_g = ((s.color >>  8) & 0xFF) as u32;
                let src_b = ( s.color        & 0xFF) as u32;

                for row in s.y..y_end {
                    let row_base = (row * stride) as usize;
                    for col in s.x..x_end {
                        let dst = buf[row_base + col as usize];
                        buf[row_base + col as usize] =
                            alpha_blend(src_r, src_g, src_b, src_a, dst);
                    }
                }
            }
        }
    }

    // ---- pixel-buffer blit -------------------------------------------------

    fn paint_pixels(&self, buf: &mut [u32], stride: u32, s: &Surface, pix: SurfacePixels) {
        let x_end = (s.x + s.width).min(self.width);
        let y_end = (s.y + s.height).min(self.height);

        let clip_w = x_end - s.x;
        let clip_h = y_end - s.y;

        match s.blend {
            BlendMode::Opaque => {
                // Fast path: copy whole scanlines when widths match and
                // the surface is fully on-screen.
                if clip_w == s.width {
                    for row in 0..clip_h {
                        let dst_base = ((s.y + row) * stride + s.x) as usize;
                        let src_base = (row * pix.stride) as usize;
                        // SAFETY: caller guarantees pix.ptr is valid for
                        //         pix.stride * s.height pixels.
                        let src_row = unsafe {
                            core::slice::from_raw_parts(
                                pix.ptr.add(src_base),
                                clip_w as usize,
                            )
                        };
                        buf[dst_base..dst_base + clip_w as usize]
                            .copy_from_slice(src_row);
                    }
                } else {
                    // Clipped: copy pixel-by-pixel.
                    for row in 0..clip_h {
                        let dst_base = ((s.y + row) * stride) as usize;
                        let src_base = (row * pix.stride) as usize;
                        for col in 0..clip_w {
                            // SAFETY: bounds checked above.
                            let src_px = unsafe {
                                pix.ptr.add(src_base + col as usize).read()
                            };
                            buf[dst_base + (s.x + col) as usize] = src_px;
                        }
                    }
                }
            }
            BlendMode::Alpha => {
                // Per-pixel alpha blend: use each source pixel's own alpha.
                for row in 0..clip_h {
                    let dst_base = ((s.y + row) * stride) as usize;
                    let src_base = (row * pix.stride) as usize;
                    for col in 0..clip_w {
                        // SAFETY: bounds checked above.
                        let src_px = unsafe {
                            pix.ptr.add(src_base + col as usize).read()
                        };
                        let src_a = (src_px >> 24) & 0xFF;
                        // Fully transparent — skip write entirely.
                        if src_a == 0 { continue; }
                        // Fully opaque — fast path.
                        let dst_idx = dst_base + (s.x + col) as usize;
                        if src_a == 255 {
                            buf[dst_idx] = src_px;
                            continue;
                        }
                        let src_r = (src_px >> 16) & 0xFF;
                        let src_g = (src_px >>  8) & 0xFF;
                        let src_b =  src_px        & 0xFF;
                        let dst   = buf[dst_idx];
                        buf[dst_idx] = alpha_blend(src_r, src_g, src_b, src_a, dst);
                    }
                }
            }
        }
    }

    /// Screen dimensions.
    #[inline]
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
