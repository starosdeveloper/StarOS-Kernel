//! Input FFI — Kotlin/Native bindings for input system
//!
//! Exports input event handling with C ABI:
//! - Mouse events (move, button, wheel)
//! - Keyboard events (key down/up)
//! - Touch events (multi-touch)
//! - Event queue management
//! - Cursor control
//! - Event listeners/callbacks

use super::types::*;
use crate::input::{self, InputEvent, EventQueue};
use core::ffi::c_void;

// ---------------------------------------------------------------------------
// Event conversion helpers
// ---------------------------------------------------------------------------

fn input_event_to_ffi(event: InputEvent) -> FFIInputEvent {
    match event {
        InputEvent::MouseMove { dx, dy } => FFIInputEvent {
            event_type: FFIInputEventType::MouseMove,
            data: FFIInputEventData {
                mouse_move: FFIMouseMoveData { dx, dy },
            },
        },
        InputEvent::MouseButton { buttons, pressed, x, y } => FFIInputEvent {
            event_type: FFIInputEventType::MouseButton,
            data: FFIInputEventData {
                mouse_button: FFIMouseButtonData { buttons, pressed, x, y },
            },
        },
        InputEvent::MouseWheel { delta, x, y } => FFIInputEvent {
            event_type: FFIInputEventType::MouseWheel,
            data: FFIInputEventData {
                mouse_wheel: FFIMouseWheelData { delta, x, y },
            },
        },
        InputEvent::KeyDown { scancode } => FFIInputEvent {
            event_type: FFIInputEventType::KeyDown,
            data: FFIInputEventData {
                key: FFIKeyData { scancode },
            },
        },
        InputEvent::KeyUp { scancode } => FFIInputEvent {
            event_type: FFIInputEventType::KeyUp,
            data: FFIInputEventData {
                key: FFIKeyData { scancode },
            },
        },
        InputEvent::Touch { id, x, y, pressed } => FFIInputEvent {
            event_type: FFIInputEventType::Touch,
            data: FFIInputEventData {
                touch: FFITouchData { id, x, y, pressed },
            },
        },
    }
}

// ---------------------------------------------------------------------------
// Input manager initialization
// ---------------------------------------------------------------------------

/// Initialize the input manager
#[no_mangle]
pub extern "C" fn staros_input_init() -> FFIError {
    // Input manager is typically initialized automatically
    // This is a placeholder for explicit initialization if needed
    FFIError::Success
}

// ---------------------------------------------------------------------------
// Event queue operations
// ---------------------------------------------------------------------------

/// Poll for pending input events
///
/// Fills `events` buffer with up to `max_events` pending events.
/// Returns the number of events written.
///
/// # Safety
/// - `events` must point to valid buffer of at least `max_events` size
#[no_mangle]
pub unsafe extern "C" fn staros_input_poll_events(
    events: *mut FFIInputEvent,
    max_events: usize,
) -> usize {
    if events.is_null() || max_events == 0 {
        return 0;
    }

    let mut count = 0;
    let events_slice = core::slice::from_raw_parts_mut(events, max_events);

    while count < max_events {
        match input::EVENT_QUEUE.pop() {
            Some(event) => {
                events_slice[count] = input_event_to_ffi(event);
                count += 1;
            }
            None => break,
        }
    }

    count
}

/// Check if there are pending input events
#[no_mangle]
pub extern "C" fn staros_input_has_events() -> bool {
    !input::EVENT_QUEUE.is_empty()
}

/// Get number of pending events in queue
#[no_mangle]
pub extern "C" fn staros_input_event_count() -> usize {
    // Event queue doesn't expose count, so we return 0 if empty, 1 otherwise
    if input::EVENT_QUEUE.is_empty() { 0 } else { 1 }
}

/// Push an event to the input queue (for driver/IRQ use)
///
/// Returns true if event was queued, false if queue is full
#[no_mangle]
pub extern "C" fn staros_input_push_event(event: FFIInputEvent) -> bool {
    // Convert FFI event back to internal format
    let internal_event = unsafe {
        match event.event_type {
            FFIInputEventType::MouseMove => {
                let data = event.data.mouse_move;
                InputEvent::MouseMove { dx: data.dx, dy: data.dy }
            }
            FFIInputEventType::MouseButton => {
                let data = event.data.mouse_button;
                InputEvent::MouseButton {
                    buttons: data.buttons,
                    pressed: data.pressed,
                    x: data.x,
                    y: data.y,
                }
            }
            FFIInputEventType::MouseWheel => {
                let data = event.data.mouse_wheel;
                InputEvent::MouseWheel {
                    delta: data.delta,
                    x: data.x,
                    y: data.y,
                }
            }
            FFIInputEventType::KeyDown => {
                let data = event.data.key;
                InputEvent::KeyDown { scancode: data.scancode }
            }
            FFIInputEventType::KeyUp => {
                let data = event.data.key;
                InputEvent::KeyUp { scancode: data.scancode }
            }
            FFIInputEventType::Touch => {
                let data = event.data.touch;
                InputEvent::Touch {
                    id: data.id,
                    x: data.x,
                    y: data.y,
                    pressed: data.pressed,
                }
            }
        }
    };

    input::EVENT_QUEUE.push(internal_event)
}

// ---------------------------------------------------------------------------
// Mouse/cursor control
// ---------------------------------------------------------------------------

/// Get current cursor position
#[no_mangle]
pub extern "C" fn staros_input_get_cursor_pos() -> FFIResult<FFICursor> {
    let manager = input::INPUT_MANAGER.lock();
    let (x, y) = manager.cursor_pos();
    let cursor = FFICursor {
        x,
        y,
        visible: true, // InputManager doesn't track visibility
        color: 0xFFFFFFFF,
        size: 16,
    };

    FFIResult::ok(cursor)
}

/// Set cursor position
#[no_mangle]
pub extern "C" fn staros_input_set_cursor_pos(_x: u32, _y: u32) -> FFIError {
    // InputManager doesn't have set_cursor_position
    // Cursor moves via MouseMove events
    FFIError::Success
}

/// Set cursor visibility
#[no_mangle]
pub extern "C" fn staros_input_set_cursor_visible(_visible: bool) -> FFIError {
    // Cursor visibility controlled by display server surface
    FFIError::Success
}

/// Move cursor by relative offset
#[no_mangle]
pub extern "C" fn staros_input_move_cursor(dx: i16, dy: i16) -> FFIError {
    let mut manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return FFIError::NotInitialized,
    };

    manager.move_cursor(dx, dy);
    FFIError::Success
}

// ---------------------------------------------------------------------------
// Touch input
// ---------------------------------------------------------------------------

/// Get active touch points
///
/// Fills `touches` buffer with up to `max_touches` active touch points.
/// Returns the number of touches written.
///
/// # Safety
/// - `touches` must point to valid buffer of at least `max_touches` size
#[no_mangle]
pub unsafe extern "C" fn staros_input_get_touches(
    touches: *mut FFITouchData,
    max_touches: usize,
) -> usize {
    if touches.is_null() || max_touches == 0 {
        return 0;
    }

    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return 0,
    };

    let active_touches = manager.active_touches();
    let count = active_touches.len().min(max_touches);
    let touches_slice = core::slice::from_raw_parts_mut(touches, max_touches);

    for (i, touch) in active_touches.iter().take(count).enumerate() {
        touches_slice[i] = FFITouchData {
            id: touch.id,
            x: touch.x,
            y: touch.y,
            pressed: touch.pressed,
        };
    }

    count
}

/// Get number of active touch points
#[no_mangle]
pub extern "C" fn staros_input_touch_count() -> usize {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return 0,
    };

    manager.active_touch_count()
}

// ---------------------------------------------------------------------------
// Keyboard state
// ---------------------------------------------------------------------------

/// Check if a key is currently pressed
#[no_mangle]
pub extern "C" fn staros_input_is_key_pressed(scancode: u16) -> bool {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return false,
    };

    manager.is_key_pressed(scancode)
}

/// Get all currently pressed keys
///
/// Fills `scancodes` buffer with up to `max_keys` pressed key scancodes.
/// Returns the number of keys written.
///
/// # Safety
/// - `scancodes` must point to valid buffer of at least `max_keys` size
#[no_mangle]
pub unsafe extern "C" fn staros_input_get_pressed_keys(
    scancodes: *mut u16,
    max_keys: usize,
) -> usize {
    if scancodes.is_null() || max_keys == 0 {
        return 0;
    }

    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return 0,
    };

    let pressed_keys = manager.pressed_keys();
    let count = pressed_keys.len().min(max_keys);
    let scancodes_slice = core::slice::from_raw_parts_mut(scancodes, max_keys);

    for (i, &scancode) in pressed_keys.iter().take(count).enumerate() {
        scancodes_slice[i] = scancode;
    }

    count
}

// ---------------------------------------------------------------------------
// Mouse button state
// ---------------------------------------------------------------------------

/// Get current mouse button state (bitmask)
#[no_mangle]
pub extern "C" fn staros_input_get_mouse_buttons() -> u8 {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return 0,
    };

    manager.mouse_buttons()
}

/// Check if specific mouse button is pressed
#[no_mangle]
pub extern "C" fn staros_input_is_mouse_button_pressed(button: u8) -> bool {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return false,
    };

    (manager.mouse_buttons() & button) != 0
}

// ---------------------------------------------------------------------------
// Event listeners/callbacks
// ---------------------------------------------------------------------------

/// Register a global event listener callback
///
/// The callback will be invoked for all input events.
///
/// # Safety
/// - `callback` must be a valid function pointer
/// - `user_data` must remain valid for the lifetime of the listener
#[no_mangle]
pub unsafe extern "C" fn staros_input_register_listener(
    callback: FFIEventCallback,
    user_data: *mut c_void,
) -> FFIResult<usize> {
    let mut manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    // Wrap the FFI callback
    let wrapped_callback = move |surface_handle: usize, event: InputEvent| {
        let ffi_event = input_event_to_ffi(event);
        callback(surface_handle, ffi_event, user_data);
    };

    match manager.register_listener(wrapped_callback) {
        Some(handle) => FFIResult::ok(handle),
        None => FFIResult::err(FFIError::ResourceExhausted),
    }
}

/// Unregister an event listener
#[no_mangle]
pub extern "C" fn staros_input_unregister_listener(handle: usize) -> FFIError {
    let mut manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return FFIError::NotInitialized,
    };

    manager.unregister_listener(handle);
    FFIError::Success
}

/// Register a surface-specific event listener
///
/// The callback will be invoked only for events that hit the specified surface.
///
/// # Safety
/// - `callback` must be a valid function pointer
/// - `user_data` must remain valid for the lifetime of the listener
#[no_mangle]
pub unsafe extern "C" fn staros_input_register_surface_listener(
    surface_handle: usize,
    callback: FFIEventCallback,
    user_data: *mut c_void,
) -> FFIResult<usize> {
    let mut manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let wrapped_callback = move |handle: usize, event: InputEvent| {
        let ffi_event = input_event_to_ffi(event);
        callback(handle, ffi_event, user_data);
    };

    match manager.register_surface_listener(surface_handle, wrapped_callback) {
        Some(handle) => FFIResult::ok(handle),
        None => FFIResult::err(FFIError::ResourceExhausted),
    }
}

// ---------------------------------------------------------------------------
// Hit testing
// ---------------------------------------------------------------------------

/// Perform hit test at screen coordinates
///
/// Returns the handle of the topmost surface at (x, y), or -1 if none.
#[no_mangle]
pub extern "C" fn staros_input_hit_test(x: u32, y: u32) -> isize {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return -1,
    };

    match manager.hit_test(x, y) {
        Some(handle) => handle as isize,
        None => -1,
    }
}

// ---------------------------------------------------------------------------
// Gesture recognition (basic)
// ---------------------------------------------------------------------------

/// Detect if a swipe gesture occurred
///
/// Returns true if a swipe was detected in the last frame.
/// Direction is encoded in `direction`: 0=up, 1=right, 2=down, 3=left
#[no_mangle]
pub extern "C" fn staros_input_detect_swipe(direction: *mut u8) -> bool {
    if direction.is_null() {
        return false;
    }

    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return false,
    };

    match manager.detect_swipe() {
        Some(dir) => {
            unsafe {
                *direction = dir;
            }
            true
        }
        None => false,
    }
}

/// Detect if a pinch gesture occurred
///
/// Returns true if a pinch was detected.
/// `scale` is set to the pinch scale factor (>1.0 = zoom in, <1.0 = zoom out)
#[no_mangle]
pub extern "C" fn staros_input_detect_pinch(scale: *mut f32) -> bool {
    if scale.is_null() {
        return false;
    }

    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return false,
    };

    match manager.detect_pinch() {
        Some(s) => {
            unsafe {
                *scale = s;
            }
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Input device info
// ---------------------------------------------------------------------------

/// Check if mouse is available
#[no_mangle]
pub extern "C" fn staros_input_has_mouse() -> bool {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return false,
    };

    manager.has_mouse()
}

/// Check if keyboard is available
#[no_mangle]
pub extern "C" fn staros_input_has_keyboard() -> bool {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return false,
    };

    manager.has_keyboard()
}

/// Check if touchscreen is available
#[no_mangle]
pub extern "C" fn staros_input_has_touchscreen() -> bool {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return false,
    };

    manager.has_touchscreen()
}

/// Get maximum number of simultaneous touch points supported
#[no_mangle]
pub extern "C" fn staros_input_max_touch_points() -> usize {
    let manager = match Some(input::INPUT_MANAGER.lock()) {
        Some(m) => m,
        None => return 0,
    };

    manager.max_touch_points()
}
