// SPDX-License-Identifier: MIT
//! Display Server — video memory manager and output abstraction.
//!
//! # Architecture
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────────────────┐
//!  │  DisplayServer                                                       │
//!  │                                                                      │
//!  │   Compositor ──► DoubleBuffer ──► DmaFlip ──► Framebuffer (HW)      │
//!  │       │                │              │                              │
//!  │  Surface table    back_buffer    async cookie        HW scanout      │
//!  │  (z-ordered)      (kernel RAM)   (DMA in-flight)                    │
//!  └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Frame rendering modes
//!
//! ## Synchronous (default, no DMA)
//! ```ignore
//! ds.present();  // compose + CPU copy → blocks until copy done
//! ```
//!
//! ## Asynchronous (DMA-accelerated)
//! ```ignore
//! // Frame N:
//! let cookie = ds.present_async(ops, chan, tracker)?;
//!
//! // While DMA copies frame N to video RAM, CPU renders frame N+1:
//! ds.compose_next();
//!
//! // Before submitting frame N+1, ensure N is done:
//! ds.wait_flip(cookie, ops, chan);
//! let cookie2 = ds.present_async(ops, chan, tracker)?;
//! ```

pub mod compositor;
pub mod dma_flip;
pub mod text_surface;

pub use compositor::{Surface, BlendMode};
pub use dma_flip::{FlipCookie, DmaFlipError};
pub use text_surface::TextSurface;

use spin::Mutex;

use crate::drivers::display::{
    Framebuffer, DoubleBuffer, PixelFormat, MAX_FB_PIXELS,
};
use crate::drivers::dma::engine::{
    DmaChannel, DmaDeviceOps, DmaCookieTracker,
};
use compositor::Compositor;
use dma_flip::DmaFlip;
/// Maximum number of TextSurfaces the display server can hold.
pub const MAX_TEXT_SURFACES: usize = 16;

// ---------------------------------------------------------------------------
// Static back-buffer storage (no heap allocator required)
// ---------------------------------------------------------------------------

/// Static storage for the software back-buffer.
/// `MAX_FB_PIXELS` covers up to 1920×1080 (2 073 600 pixels = 8 MB).
struct BackBufferCell(core::cell::UnsafeCell<[u32; MAX_FB_PIXELS]>);
unsafe impl Sync for BackBufferCell {}
static BACK_BUFFER_STORAGE: BackBufferCell = BackBufferCell(core::cell::UnsafeCell::new([0u32; MAX_FB_PIXELS]));

// ---------------------------------------------------------------------------
// Global display server instance
// ---------------------------------------------------------------------------

static DISPLAY_SERVER: Mutex<Option<DisplayServer>> = Mutex::new(None);

/// Initialise the display server.
///
/// Must be called exactly **once** during kernel boot, after the MMU is
/// active and the framebuffer physical address is known.
///
/// # Safety
/// * `fb_addr` must be a valid, mapped framebuffer physical address.
/// * Must not be called from an interrupt context.
pub unsafe fn init(
    fb_addr: usize,
    width: u32,
    height: u32,
    stride: u32,
) -> Result<(), DisplayError> {
    let mut lock = DISPLAY_SERVER.lock();
    if lock.is_some() {
        return Err(DisplayError::AlreadyInitialised);
    }

    let fb = Framebuffer::new(fb_addr, width, height, stride, PixelFormat::Argb8888);
    // SAFETY: init() is called once at boot, exclusive access guaranteed by Mutex above
    let bb_ptr = unsafe { &mut *BACK_BUFFER_STORAGE.0.get() };
    let db = DoubleBuffer::new(&fb, bb_ptr)
        .map_err(|_| DisplayError::BufferTooSmall)?;
    let comp = Compositor::new(width, height);

    // Physical address of the back-buffer.
    let bb_phys = BACK_BUFFER_STORAGE.0.get() as u64;
    let transfer_len = (stride * height) as usize * core::mem::size_of::<u32>();
    let dma = DmaFlip::new(fb_addr as u64, bb_phys, transfer_len);

    const NONE_TS: Option<TextSurface> = None;
    *lock = Some(DisplayServer {
        fb,
        db,
        compositor: comp,
        dma,
        fb_phys: fb_addr as u64,
        text_surfaces: [NONE_TS; MAX_TEXT_SURFACES],
    });
    Ok(())
}

/// Obtain a locked reference to the global `DisplayServer`.
/// Returns `None` if `init()` has not been called yet.
pub fn get() -> Option<spin::MutexGuard<'static, Option<DisplayServer>>> {
    let guard = DISPLAY_SERVER.lock();
    if guard.is_some() { Some(guard) } else { None }
}

// ---------------------------------------------------------------------------
// DisplayServer
// ---------------------------------------------------------------------------

pub struct DisplayServer {
    fb: Framebuffer,
    db: DoubleBuffer,
    compositor: Compositor,
    /// DMA flip helper — drives async back-buffer → video RAM copies.
    dma: DmaFlip,
    /// Physical address of the hardware framebuffer (for DMA).
    fb_phys: u64,
    /// Text surfaces rendered each frame before the compositor flip.
    text_surfaces: [Option<TextSurface>; MAX_TEXT_SURFACES],
}

impl DisplayServer {
    // -----------------------------------------------------------------------
    // Surface management
    // -----------------------------------------------------------------------

    pub fn add_surface(&mut self, surface: Surface) -> Option<usize> {
        self.compositor.add_surface(surface)
    }

    pub fn remove_surface(&mut self, handle: usize) {
        self.compositor.remove_surface(handle);
    }

    pub fn surface_mut(&mut self, handle: usize) -> Option<&mut Surface> {
        self.compositor.surface_mut(handle)
    }

    /// Read-only access to a surface (for hit-testing).
    pub fn surface_ref(&self, handle: usize) -> Option<&Surface> {
        self.compositor.surface_ref(handle)
    }

    pub fn set_background(&mut self, color: u32) {
        self.compositor.set_background(color);
    }

    // -----------------------------------------------------------------------
    // Text surface management
    // -----------------------------------------------------------------------

    /// Add a [`TextSurface`] and return its handle.
    /// Returns `None` if the table is full.
    pub fn add_text_surface(&mut self, ts: TextSurface) -> Option<usize> {
        for (i, slot) in self.text_surfaces.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(ts);
                return Some(i);
            }
        }
        None
    }

    /// Remove a text surface by handle.
    pub fn remove_text_surface(&mut self, handle: usize) {
        if handle < MAX_TEXT_SURFACES {
            self.text_surfaces[handle] = None;
        }
    }

    /// Mutably access a text surface (to update its text or style).
    pub fn text_surface_mut(&mut self, handle: usize) -> Option<&mut TextSurface> {
        self.text_surfaces.get_mut(handle)?.as_mut()
    }

    // -----------------------------------------------------------------------
    // Rendering — synchronous path (CPU copy)
    // -----------------------------------------------------------------------

    /// Compose all surfaces into the back-buffer, then CPU-copy to video RAM.
    ///
    /// Simple and always correct.  Use `present_async` for better throughput.
    ///
    /// # Safety
    /// No other code must be writing to the hardware framebuffer concurrently.
    pub unsafe fn present(&mut self) {
        self.do_compose();
        self.db.flip(&mut self.fb);
    }

    /// Clear the screen to `color` (sync, bypasses compositor).
    pub unsafe fn clear(&mut self, color: u32) {
        self.db.clear(color);
        self.db.flip(&mut self.fb);
    }

    // -----------------------------------------------------------------------
    // Rendering — asynchronous path (DMA)
    // -----------------------------------------------------------------------

    /// Compose surfaces into the back-buffer, then kick off a DMA transfer
    /// to copy the finished frame to video RAM in the background.
    ///
    /// Returns a `FlipCookie` the caller must pass to `wait_flip()` before
    /// submitting the next frame.
    ///
    /// # Typical double-buffered loop
    /// ```ignore
    /// let cookie = ds.present_async(ops, chan, tracker)?;
    /// ds.compose_next_frame();          // CPU renders N+1 while DMA copies N
    /// ds.wait_flip(cookie, ops, chan);  // ensure N landed before next flip
    /// ```
    ///
    /// # Safety
    /// No other code must be writing to the hardware framebuffer concurrently.
    pub unsafe fn present_async(
        &mut self,
        ops: &dyn DmaDeviceOps,
        chan: &DmaChannel,
        tracker: &DmaCookieTracker,
    ) -> Result<FlipCookie, DmaFlipError> {
        // Guard: refuse if a previous flip is still in flight.
        if self.dma.is_busy() {
            return Err(DmaFlipError::Busy);
        }

        // Step 1: CPU renders into back-buffer.
        self.do_compose();

        // Step 2: hand the copy off to the DMA engine.
        self.dma.submit(ops, chan, tracker)
    }

    /// Register an IRQ-driven callback for a DMA flip instead of polling.
    ///
    /// The callback is invoked from the DMA IRQ handler when the transfer
    /// completes.  The `u32` argument is the raw cookie value.
    pub fn on_flip_complete(
        &self,
        cookie: FlipCookie,
        tracker: &DmaCookieTracker,
        callback: crate::drivers::dma::engine::DmaCallback,
    ) {
        self.dma.register_callback(cookie, tracker, callback);
    }

    /// Block (busy-poll) until the DMA flip identified by `cookie` is done.
    ///
    /// Replace the spin with a scheduler yield once the process manager
    /// is integrated.
    pub fn wait_flip(
        &mut self,
        cookie: FlipCookie,
        ops: &dyn DmaDeviceOps,
        chan: &DmaChannel,
    ) {
        self.dma.wait_complete(cookie, ops, chan);
    }

    /// Check (non-blocking) whether the flip is complete.
    pub fn flip_done(
        &self,
        cookie: FlipCookie,
        ops: &dyn DmaDeviceOps,
        chan: &DmaChannel,
    ) -> bool {
        self.dma.is_complete(cookie, ops, chan)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Run the compositor — paint all surfaces into the back-buffer.
    ///
    /// # Safety
    /// Must only be called while holding exclusive access to `self`.
    unsafe fn do_compose(&mut self) {
        let stride = self.fb.stride;
        let height = self.fb.height;
        let ptr = self.db.back_buffer().as_ptr() as *mut u32;
        let len = (stride * height) as usize;
        let back = core::slice::from_raw_parts_mut(ptr, len);

        // 1. Compositor paints all Surfaces (background, rects, etc.).
        self.compositor.compose(back, stride);

        // 2. Text surfaces are rendered on top (they respect their own z_order
        //    relative to other text, but always above the compositor layer).
        //    For proper z-mixing with Surfaces, integrate TextSurface into the
        //    compositor table in a future iteration.
        for slot in &self.text_surfaces {
            if let Some(ts) = slot {
                ts.render_into(back, stride, height);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    pub fn dimensions(&self) -> (u32, u32) {
        self.fb.dimensions()
    }

    /// Physical address of the hardware framebuffer.
    pub fn fb_phys(&self) -> u64 {
        self.fb_phys
    }

    /// `true` if a DMA flip is currently in flight.
    pub fn dma_busy(&self) -> bool {
        self.dma.is_busy()
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum DisplayError {
    AlreadyInitialised,
    BufferTooSmall,
}
