// SPDX-License-Identifier: MIT
//! Input event types and lock-free event queue.
//!
//! # Event flow
//!
//! ```text
//!  IRQ handler (mouse/touch)
//!       │
//!       ▼
//!  EventQueue::push()   ← atomic ring buffer, safe from interrupt context
//!       │
//!       ▼
//!  InputManager::poll() ← called each frame from the render loop
//!       │
//!       ├─► update cursor Surface position
//!       ├─► hit-test click → dispatch to Surface listener
//!       └─► call global listeners (keyboard shortcuts, etc.)
//! ```

use core::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// Mouse button bitmask.
pub mod btn {
    pub const LEFT:   u8 = 1 << 0;
    pub const RIGHT:  u8 = 1 << 1;
    pub const MIDDLE: u8 = 1 << 2;
}

/// A single input event produced by a device driver or IRQ handler.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// Mouse moved by `(dx, dy)` pixels (relative, signed).
    MouseMove { dx: i16, dy: i16 },

    /// Mouse button state changed.
    /// `buttons` is a bitmask of [`btn`] constants.
    /// `x`, `y` are absolute screen coordinates at the time of the event.
    MouseButton { buttons: u8, pressed: bool, x: u32, y: u32 },

    /// Mouse wheel scrolled.
    /// `delta` > 0 is scroll-up, < 0 is scroll-down.
    MouseWheel { delta: i8, x: u32, y: u32 },

    /// Raw keyboard scancode (layout-independent).
    KeyDown { scancode: u16 },
    KeyUp   { scancode: u16 },

    /// Touch event (forwarded from the touch driver).
    Touch { id: u8, x: u32, y: u32, pressed: bool },
}

// ---------------------------------------------------------------------------
// Lock-free ring-buffer event queue
// ---------------------------------------------------------------------------

/// Capacity of the static event queue.
/// Must be a power of two.
pub const QUEUE_CAPACITY: usize = 64;
const MASK: usize = QUEUE_CAPACITY - 1;

/// Lock-free single-producer / single-consumer ring buffer for input events.
///
/// **Producer** (IRQ handler): calls `push()`.
/// **Consumer** (render loop): calls `pop()`.
///
/// Safe from interrupt context — uses only atomic operations and no locks.
pub struct EventQueue {
    buf:  [InputEvent; QUEUE_CAPACITY],
    head: AtomicUsize, // consumer reads here
    tail: AtomicUsize, // producer writes here
    /// Number of oldest events dropped due to queue overflow.
    dropped_count: AtomicUsize,
}

impl EventQueue {
    pub const fn new() -> Self {
        Self {
            // Safe: InputEvent is Copy; MouseMove{0,0} is a valid sentinel.
            buf:  [InputEvent::MouseMove { dx: 0, dy: 0 }; QUEUE_CAPACITY],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            dropped_count: AtomicUsize::new(0),
        }
    }

    /// Push an event from **interrupt context** (producer side).
    ///
    /// If the queue is full, drops the oldest event to make room.
    /// Returns `false` if an oldest event was dropped.
    #[inline]
    pub fn push(&self, event: InputEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let next_tail = (tail + 1) & MASK;

        let mut dropped = false;
        if next_tail == self.head.load(Ordering::Acquire) {
            // Queue full — advance head to drop oldest event.
            let head = self.head.load(Ordering::Relaxed);
            self.head.store((head + 1) & MASK, Ordering::Release);
            self.dropped_count.fetch_add(1, Ordering::Relaxed);
            dropped = true;
        }

        // SAFETY: only the producer touches `buf[tail]`, and we hold the
        // only producer (single IRQ line or serialised through a spinlock).
        unsafe {
            let slot = &self.buf[tail] as *const InputEvent as *mut InputEvent;
            slot.write(event);
        }

        self.tail.store(next_tail, Ordering::Release);
        !dropped
    }

    /// Returns the number of events dropped due to overflow.
    pub fn dropped_count(&self) -> usize {
        self.dropped_count.load(Ordering::Relaxed)
    }

    /// Pop an event from the **render loop** (consumer side).
    #[inline]
    pub fn pop(&self) -> Option<InputEvent> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }

        // SAFETY: producer has already written and published via Release store.
        let event = unsafe {
            let slot = &self.buf[head] as *const InputEvent;
            slot.read()
        };

        self.head.store((head + 1) & MASK, Ordering::Release);
        Some(event)
    }

    /// Drain all pending events into `out` slice.
    /// Returns the number of events written.
    pub fn drain(&self, out: &mut [InputEvent]) -> usize {
        let mut n = 0;
        while n < out.len() {
            match self.pop() {
                Some(ev) => { out[n] = ev; n += 1; }
                None => break,
            }
        }
        n
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Relaxed) == self.tail.load(Ordering::Relaxed)
    }
}

// SAFETY: EventQueue uses only atomic ops; no UnsafeCell aliasing outside
// the careful producer/consumer protocol above.
unsafe impl Sync for EventQueue {}
unsafe impl Send for EventQueue {}

// ---------------------------------------------------------------------------
// Surface event listener
// ---------------------------------------------------------------------------

/// Callback registered by a Surface to receive input events routed to it.
///
/// Called from `InputManager::poll()` when a click lands inside the surface's
/// bounding box.  The `surface_handle` identifies which surface was hit.
///
/// Use a bare `fn` pointer — no closures, no alloc, no_std safe.
pub type EventListener = fn(surface_handle: usize, event: InputEvent);

/// Maximum number of per-surface listeners the InputManager tracks.
pub const MAX_LISTENERS: usize = 32;
