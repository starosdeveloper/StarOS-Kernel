//! Input FFI — Kotlin/Native bindings for input system
//!
//! Exports only implemented and working input functionality.

use super::types::*;
use crate::input::{self, InputEvent};

// ---------------------------------------------------------------------------
// Event conversion
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
// Event queue operations (WORKING)
// ---------------------------------------------------------------------------

/// Initialize input manager
#[no_mangle]
pub extern "C" fn staros_input_init() -> FFIError {
    FFIError::Success
}

/// Poll for pending input events
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

/// Push an event to the input queue
#[no_mangle]
pub extern "C" fn staros_input_push_event(event: FFIInputEvent) -> bool {
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
// Cursor control (WORKING)
// ---------------------------------------------------------------------------

/// Get current cursor position
#[no_mangle]
pub extern "C" fn staros_input_get_cursor_pos() -> FFIResult<FFICursor> {
    let manager = input::INPUT_MANAGER.lock();
    let (x, y) = manager.cursor_pos();
    
    let cursor = FFICursor {
        x,
        y,
        visible: true,
        color: 0xFFFFFFFF,
        size: 16,
    };

    FFIResult::ok(cursor)
}

/// Move cursor by relative offset
#[no_mangle]
pub extern "C" fn staros_input_move_cursor(dx: i16, dy: i16) -> FFIError {
    let event = InputEvent::MouseMove { dx, dy };
    input::EVENT_QUEUE.push(event);
    FFIError::Success
}

// ---------------------------------------------------------------------------
// Mouse button state (WORKING)
// ---------------------------------------------------------------------------

/// Get current mouse button state (bitmask)
#[no_mangle]
pub extern "C" fn staros_input_get_mouse_buttons() -> u8 {
    let manager = input::INPUT_MANAGER.lock();
    manager.held_buttons()
}

/// Check if specific mouse button is pressed
#[no_mangle]
pub extern "C" fn staros_input_is_mouse_button_pressed(button: u8) -> bool {
    let manager = input::INPUT_MANAGER.lock();
    (manager.held_buttons() & button) != 0
}
