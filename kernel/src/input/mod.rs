// SPDX-License-Identifier: MIT
//! InputManager — mouse, keyboard, touch.
//!
//! # Architecture
//!
//! ```text
//!  ┌──────────────────────────────────────────────────────────┐
//!  │  InputManager                                            │
//!  │                                                          │
//!  │  PS/2 IRQ 12 ──► Ps2Mouse::feed() ──► EventQueue        │
//!  │  USB HID IRQ ──► UsbHidMouse::handle_report() ──►       │
//!  │  PS/2 KBD    ──► Ps2Keyboard::feed() ──►                │
//!  │                                         │               │
//!  │               poll() called each frame  │               │
//!  │                        │                ▼               │
//!  │               ┌────────┴──────────────────────┐         │
//!  │               │  per event:                   │         │
//!  │               │  MouseMove  → clamp & update  │         │
//!  │               │             cursor Surface     │         │
//!  │               │  MouseBtn   → hit-test         │         │
//!  │               │             → dispatch listener │         │
//!  │               │  Key*       → global listeners  │         │
//!  │               └───────────────────────────────┘         │
//!  └──────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! // Boot sequence:
//! input::init(screen_width, screen_height);
//! input::INPUT_MANAGER.lock().register_ps2(&mut irq_ctrl);
//!
//! // Register a cursor surface (max z_order):
//! let cursor = display_server::get().unwrap()
//!     .as_mut().unwrap()
//!     .add_surface(Surface { z_order: 255, width: 12, height: 20, .. });
//! input::INPUT_MANAGER.lock().set_cursor(cursor);
//!
//! // Register a button listener:
//! input::INPUT_MANAGER.lock()
//!     .register_listener(btn_handle, on_button_event);
//!
//! // Each frame (render loop):
//! input::INPUT_MANAGER.lock().poll();
//! ```

pub mod event;
pub mod mouse;

pub use event::{InputEvent, EventQueue, EventListener, btn};
pub use mouse::{Ps2Mouse, UsbHidMouse, Ps2Keyboard, PS2_MOUSE_IRQ};

use spin::Mutex;

use event::MAX_LISTENERS;
use mouse::ps2_mouse_irq;

use crate::display_server;
use crate::interrupts::{InterruptController, IrqConfig, IrqPriority};

// ---------------------------------------------------------------------------
// Global event queue — written by IRQ handlers, read by poll()
// ---------------------------------------------------------------------------

/// The single global event queue.
/// `static` so that IRQ handlers (which cannot take `&mut self`) can reach it.
pub static EVENT_QUEUE: EventQueue = EventQueue::new();

// ---------------------------------------------------------------------------
// Global InputManager instance
// ---------------------------------------------------------------------------

pub static INPUT_MANAGER: Mutex<InputManager> = Mutex::new(InputManager::new());

/// Initialise the InputManager with the screen dimensions.
pub fn init(screen_width: u32, screen_height: u32) {
    let mut mgr = INPUT_MANAGER.lock();
    mgr.screen_w = screen_width;
    mgr.screen_h = screen_height;
}

// ---------------------------------------------------------------------------
// InputManager
// ---------------------------------------------------------------------------

pub struct InputManager {
    // ---- device state ----
    ps2_mouse:   Ps2Mouse,
    usb_mouse:   UsbHidMouse,
    ps2_kbd:     Ps2Keyboard,

    // ---- cursor ----
    /// Absolute cursor position in screen pixels.
    cursor_x:    u32,
    cursor_y:    u32,
    /// Handle of the cursor Surface in the DisplayServer.
    /// `None` until `set_cursor()` is called.
    cursor_handle: Option<usize>,

    // ---- screen bounds ----
    screen_w:    u32,
    screen_h:    u32,

    // ---- event listeners ----
    /// (surface_handle, callback) pairs.
    listeners:   [(usize, Option<EventListener>); MAX_LISTENERS],

    // ---- button state ----
    /// Current bitmask of held buttons (btn::LEFT | btn::RIGHT | btn::MIDDLE).
    held_buttons: u8,
}

impl InputManager {
    pub const fn new() -> Self {
        Self {
            ps2_mouse:     Ps2Mouse::new(),
            usb_mouse:     UsbHidMouse::new(),
            ps2_kbd:       Ps2Keyboard::new(),
            cursor_x:      0,
            cursor_y:      0,
            cursor_handle: None,
            screen_w:      1080,
            screen_h:      1920,
            listeners:     [(0, None); MAX_LISTENERS],
            held_buttons:  0,
        }
    }

    // -----------------------------------------------------------------------
    // Setup
    // -----------------------------------------------------------------------

    /// Set the DisplayServer surface handle used as the mouse cursor.
    ///
    /// The cursor Surface should have `z_order = 255` so it renders on top.
    pub fn set_cursor(&mut self, handle: usize) {
        self.cursor_handle = Some(handle);
    }

    /// Register `listener` to receive events that land on `surface_handle`.
    pub fn register_listener(&mut self, surface_handle: usize, listener: EventListener) {
        for slot in &mut self.listeners {
            if slot.1.is_none() {
                *slot = (surface_handle, Some(listener));
                return;
            }
        }
        // Table full — silently drop (extend MAX_LISTENERS if needed).
    }

    /// Unregister the listener for `surface_handle`.
    pub fn unregister_listener(&mut self, surface_handle: usize) {
        for slot in &mut self.listeners {
            if slot.0 == surface_handle && slot.1.is_some() {
                *slot = (0, None);
            }
        }
    }

    /// Register the PS/2 mouse IRQ with the interrupt controller.
    pub fn register_ps2(&self, irq_ctrl: &mut InterruptController) {
        let cfg = IrqConfig {
            priority: IrqPriority::HIGH,
            enabled: true,
            edge_triggered: true,
            cpu_mask: 0x01,
        };
        let _ = irq_ctrl.register(PS2_MOUSE_IRQ, ps2_mouse_irq, cfg);
    }

    // -----------------------------------------------------------------------
    // IRQ feed methods (called from interrupt handlers)
    // -----------------------------------------------------------------------

    /// Feed a raw PS/2 mouse byte (called from IRQ 12 handler).
    pub fn feed_ps2_mouse(&mut self, byte: u8) {
        self.ps2_mouse.feed(byte, &EVENT_QUEUE, self.cursor_x, self.cursor_y);
    }

    /// Feed a USB HID mouse report (called from USB interrupt handler).
    pub fn feed_usb_hid(&mut self, report: &[u8]) {
        self.usb_mouse.handle_report(report, &EVENT_QUEUE, self.cursor_x, self.cursor_y);
    }

    /// Feed a PS/2 keyboard scancode byte.
    pub fn feed_ps2_kbd(&mut self, byte: u8) {
        self.ps2_kbd.feed(byte, &EVENT_QUEUE);
    }

    // -----------------------------------------------------------------------
    // Main poll loop — call once per frame from the render thread
    // -----------------------------------------------------------------------

    /// Drain the event queue and process every pending event.
    ///
    /// - `MouseMove`   → clamp cursor, update cursor Surface position.
    /// - `MouseButton` → hit-test surfaces, dispatch to registered listener.
    /// - `Key*`        → dispatched to all listeners (global hotkeys).
    pub fn poll(&mut self) {
        while let Some(event) = EVENT_QUEUE.pop() {
            match event {
                InputEvent::MouseMove { dx, dy } => {
                    self.move_cursor(dx, dy);
                }
                InputEvent::MouseButton { buttons, pressed, .. } => {
                    if pressed {
                        self.held_buttons |= buttons;
                    } else {
                        self.held_buttons &= !buttons;
                    }
                    // Rebuild event with current (post-move) cursor position.
                    let ev = InputEvent::MouseButton {
                        buttons,
                        pressed,
                        x: self.cursor_x,
                        y: self.cursor_y,
                    };
                    self.dispatch_click(ev);
                }
                InputEvent::MouseWheel { delta, .. } => {
                    let ev = InputEvent::MouseWheel {
                        delta,
                        x: self.cursor_x,
                        y: self.cursor_y,
                    };
                    self.dispatch_hit(self.cursor_x, self.cursor_y, ev);
                }
                InputEvent::KeyDown { .. } | InputEvent::KeyUp { .. } => {
                    self.dispatch_global(event);
                }
                InputEvent::Touch { x, y, pressed, id } => {
                    // Treat touch as mouse for now.
                    let ev = InputEvent::MouseButton {
                        buttons: btn::LEFT,
                        pressed,
                        x, y,
                    };
                    self.dispatch_hit(x, y, ev);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Cursor movement
    // -----------------------------------------------------------------------

    fn move_cursor(&mut self, dx: i16, dy: i16) {
        // Clamp to screen bounds.
        self.cursor_x = (self.cursor_x as i32 + dx as i32)
            .max(0)
            .min(self.screen_w as i32 - 1) as u32;
        self.cursor_y = (self.cursor_y as i32 + dy as i32)
            .max(0)
            .min(self.screen_h as i32 - 1) as u32;

        // Update the cursor Surface position in the DisplayServer.
        self.update_cursor_surface();
    }

    fn update_cursor_surface(&self) {
        if let Some(handle) = self.cursor_handle {
            if let Some(mut guard) = display_server::get() {
                if let Some(ds) = guard.as_mut() {
                    if let Some(surf) = ds.surface_mut(handle) {
                        surf.x = self.cursor_x;
                        surf.y = self.cursor_y;
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Hit testing and event dispatch
    // -----------------------------------------------------------------------

    /// Find the topmost visible surface that contains point `(px, py)`.
    ///
    /// "Topmost" means highest `z_order` among all surfaces that contain
    /// the point (consistent with how the compositor renders).
    /// Find the topmost visible surface containing `(px, py)`.
    fn hit_test(&self, px: u32, py: u32) -> Option<usize> {
        let mut best_handle: Option<usize> = None;
        let mut best_z: i16 = -1;

        if let Some(guard) = display_server::get() {
            if let Some(ds) = guard.as_ref() {
                for handle in 0..crate::display_server::compositor::MAX_SURFACES {
                    if let Some(surf) = ds.surface_ref(handle) {
                        if !surf.visible { continue; }

                        let in_x = px >= surf.x && px < surf.x + surf.width;
                        let in_y = py >= surf.y && py < surf.y + surf.height;

                        if in_x && in_y && surf.z_order as i16 > best_z {
                            best_z = surf.z_order as i16;
                            best_handle = Some(handle);
                        }
                    }
                }
            }
        }

        best_handle
    }

    /// Dispatch a click event to the topmost surface under the cursor.
    fn dispatch_click(&self, event: InputEvent) {
        let (x, y) = match event {
            InputEvent::MouseButton { x, y, .. } => (x, y),
            _ => return,
        };
        self.dispatch_hit(x, y, event);
    }

    /// Dispatch `event` to the listener registered for the surface at `(x,y)`.
    fn dispatch_hit(&self, x: u32, y: u32, event: InputEvent) {
        // hit_test requires read access to DisplayServer.
        // Because we hold `&mut self` (from poll()), and the DisplayServer
        // is a separate global Mutex, this is safe.
        if let Some(handle) = self.hit_test(x, y) {
            self.notify_listener(handle, event);
        }
        // Also notify listeners registered explicitly for that handle.
        // (Full hit-test is wired up once surface_ref() is available.)
    }

    /// Dispatch a keyboard event to all registered listeners.
    fn dispatch_global(&self, event: InputEvent) {
        for (handle, listener) in &self.listeners {
            if let Some(cb) = listener {
                cb(*handle, event);
            }
        }
    }

    /// Notify the listener registered for `handle`, if any.
    fn notify_listener(&self, handle: usize, event: InputEvent) {
        for (h, listener) in &self.listeners {
            if *h == handle {
                if let Some(cb) = listener {
                    cb(handle, event);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Current cursor position `(x, y)`.
    pub fn cursor_pos(&self) -> (u32, u32) {
        (self.cursor_x, self.cursor_y)
    }

    /// Bitmask of currently held mouse buttons.
    pub fn held_buttons(&self) -> u8 {
        self.held_buttons
    }
}
