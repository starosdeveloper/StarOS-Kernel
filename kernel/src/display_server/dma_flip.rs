// SPDX-License-Identifier: MIT
//! Asynchronous DMA-accelerated framebuffer flip.
//!
//! Replaces the synchronous `blit_full()` loop with a DMA MemToMem
//! transaction.  While the DMA engine copies 8 MB of framebuffer data
//! in the background, the compositor is free to render the next frame
//! into the back-buffer.
//!
//! # Lifecycle of one frame
//!
//! ```text
//!  compositor.compose()  ──► back_buffer ready
//!          │
//!          ▼
//!  DmaFlip::submit()
//!    device_prep_dma_memcpy(fb_phys, bb_phys, len)
//!    device_issue_pending()
//!    returns cookie
//!          │
//!          ▼  (hardware copies in background)
//!
//!  compositor.compose()  ◄── NEXT frame renders here concurrently
//!          │
//!          ▼
//!  DmaFlip::wait_complete(cookie)   ← poll before next submit
//!    device_tx_status() == Complete
//! ```
//!
//! # Physical addresses
//!
//! DMA operates on **physical** addresses.  The caller must supply:
//! - `fb_phys`  — physical address of the hardware framebuffer
//!   (same value passed to `display_server::init()`).
//! - `bb_phys`  — physical address of the static back-buffer array.
//!   On bare-metal with identity-mapped kernel BSS this equals the
//!   virtual address (`&BACK_BUFFER_STORAGE as *const _ as u64`).

use crate::drivers::dma::engine::{
    DmaChannel, DmaDeviceOps, DmaStatus, DmaCookieTracker,
};

/// State tracked between `submit()` and `wait_complete()`.
#[derive(Debug, Clone, Copy)]
pub struct FlipCookie(pub u32);

/// DMA-accelerated page-flip helper.
///
/// One instance lives inside `DisplayServer`.  It remembers the physical
/// addresses of both buffers so callers do not have to recompute them
/// on every frame.
pub struct DmaFlip {
    /// Physical address of the hardware framebuffer (destination).
    fb_phys: u64,
    /// Physical address of the software back-buffer (source).
    bb_phys: u64,
    /// Number of bytes to transfer (stride × height × 4).
    transfer_len: usize,
    /// Cookie of the in-flight DMA transaction, if any.
    pending: Option<FlipCookie>,
}

impl DmaFlip {
    /// Create a new flip helper.
    ///
    /// * `fb_phys`      — physical address of the video framebuffer.
    /// * `bb_phys`      — physical address of the back-buffer.
    /// * `transfer_len` — total bytes to copy (`stride * height * 4`).
    pub const fn new(fb_phys: u64, bb_phys: u64, transfer_len: usize) -> Self {
        Self {
            fb_phys,
            bb_phys,
            transfer_len,
            pending: None,
        }
    }

    // -----------------------------------------------------------------------
    // Async path
    // -----------------------------------------------------------------------

    /// Submit an asynchronous DMA flip.
    ///
    /// Prepares a MemToMem descriptor, issues it to the DMA channel, and
    /// returns a `FlipCookie` the caller can use to poll for completion.
    ///
    /// Returns `Err` if the DMA engine rejected the descriptor.
    pub fn submit(
        &mut self,
        ops: &dyn DmaDeviceOps,
        chan: &DmaChannel,
        tracker: &DmaCookieTracker,
    ) -> Result<FlipCookie, DmaFlipError> {
        // Prevent double-submit while a flip is still in flight.
        if self.pending.is_some() {
            return Err(DmaFlipError::Busy);
        }

        // Prepare descriptor: back-buffer → framebuffer.
        let desc = ops
            .device_prep_dma_memcpy(self.fb_phys, self.bb_phys, self.transfer_len)
            .map_err(DmaFlipError::PrepFailed)?;

        // Assign a tracking cookie and enqueue.
        let cookie = chan.submit_descriptor(desc, tracker);

        // Issue to hardware — DMA starts here.
        ops.device_issue_pending(chan);

        let fc = FlipCookie(cookie);
        self.pending = Some(fc);
        Ok(fc)
    }

    /// Poll whether a previously submitted flip has completed.
    ///
    /// Returns `true` when the DMA engine signals `Complete`.
    /// Non-blocking — call in a loop or from a timer/interrupt context.
    pub fn is_complete(
        &self,
        cookie: FlipCookie,
        ops: &dyn DmaDeviceOps,
        chan: &DmaChannel,
    ) -> bool {
        matches!(
            ops.device_tx_status(chan, cookie.0),
            DmaStatus::Complete
        )
    }

    /// Block until the pending flip is done (busy-poll).
    ///
    /// In a real kernel this would yield to the scheduler or sleep on an
    /// IRQ.  For now it spins — replace with your wait_queue primitive
    /// once the scheduler is up.
    pub fn wait_complete(
        &mut self,
        cookie: FlipCookie,
        ops: &dyn DmaDeviceOps,
        chan: &DmaChannel,
    ) {
        while !self.is_complete(cookie, ops, chan) {
            core::hint::spin_loop();
        }
        self.pending = None;
    }

    /// Register an IRQ-driven completion callback instead of polling.
    ///
    /// `callback` is called from the DMA IRQ handler when the transfer
    /// finishes.  The `cookie` value passed to it matches `FlipCookie.0`.
    pub fn register_callback(
        &self,
        cookie: FlipCookie,
        tracker: &DmaCookieTracker,
        callback: crate::drivers::dma::engine::DmaCallback,
    ) {
        tracker.register_callback(cookie.0, callback);
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Returns the cookie of the in-flight transaction, if any.
    pub fn pending_cookie(&self) -> Option<FlipCookie> {
        self.pending
    }

    /// `true` if a DMA transfer is currently in flight.
    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum DmaFlipError {
    /// `device_prep_dma_memcpy` returned an errno.
    PrepFailed(i32),
    /// A previous flip has not completed yet.
    Busy,
}
