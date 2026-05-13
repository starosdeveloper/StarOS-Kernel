//! Display FFI — Kotlin/Native bindings for display server
//!
//! Exports all display server functionality with C ABI:
//! - Surface management (add, remove, update, z-order)
//! - Text rendering
//! - Synchronous and asynchronous presentation
//! - DMA operations
//! - Compositor control

use super::types::*;
use crate::display_server::{self, Surface, TextSurface, BlendMode};
use crate::drivers::dma::engine::{DmaChannel, DmaDeviceOps, DmaCookieTracker};
use core::slice;

// ---------------------------------------------------------------------------
// Display server initialization
// ---------------------------------------------------------------------------

/// Initialize the display server
///
/// # Safety
/// - `fb_addr` must be a valid, mapped framebuffer physical address
/// - Must be called exactly once during kernel boot
/// - Must not be called from interrupt context
#[no_mangle]
pub unsafe extern "C" fn staros_display_init(
    fb_addr: u64,
    width: u32,
    height: u32,
    stride: u32,
) -> FFIError {
    match display_server::init(fb_addr as usize, width, height, stride) {
        Ok(()) => FFIError::Success,
        Err(e) => FFIError::from_display_error(e),
    }
}

/// Get display information
#[no_mangle]
pub extern "C" fn staros_display_get_info() -> FFIResult<FFIDisplayInfo> {
    let guard = match display_server::get() {
        Some(g) => g,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let ds = match guard.as_ref() {
        Some(ds) => ds,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let (width, height) = ds.dimensions();
    let info = FFIDisplayInfo {
        width,
        height,
        stride: width,
        fb_phys: ds.fb_phys(),
        dma_busy: ds.dma_busy(),
    };

    FFIResult::ok(info)
}

// ---------------------------------------------------------------------------
// Surface management
// ---------------------------------------------------------------------------

/// Add a surface to the compositor
#[no_mangle]
pub extern "C" fn staros_display_add_surface(surface: FFISurface) -> FFIResult<usize> {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let pixels = if surface.pixels.is_null() {
        None
    } else {
        Some(crate::display_server::compositor::SurfacePixels {
            ptr: surface.pixels,
            stride: surface.stride,
        })
    };

    let blend = match surface.blend_mode {
        FFIBlendMode::Opaque => BlendMode::Opaque,
        FFIBlendMode::Alpha => BlendMode::Alpha,
    };

    let surf = Surface {
        x: surface.x,
        y: surface.y,
        width: surface.width,
        height: surface.height,
        color: surface.color,
        pixels,
        blend,
        z_order: surface.z_order,
        visible: surface.visible,
    };

    match ds.add_surface(surf) {
        Some(handle) => FFIResult::ok(handle),
        None => FFIResult::err(FFIError::ResourceExhausted),
    }
}

/// Remove a surface from the compositor
#[no_mangle]
pub extern "C" fn staros_display_remove_surface(handle: usize) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    ds.remove_surface(handle);
    FFIError::Success
}

/// Update surface properties
#[no_mangle]
pub extern "C" fn staros_display_update_surface(
    handle: usize,
    surface: FFISurface,
) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    let surf = match ds.surface_mut(handle) {
        Some(s) => s,
        None => return FFIError::NotFound,
    };

    surf.x = surface.x;
    surf.y = surface.y;
    surf.width = surface.width;
    surf.height = surface.height;
    surf.color = surface.color;
    surf.z_order = surface.z_order;
    surf.visible = surface.visible;

    surf.blend = match surface.blend_mode {
        FFIBlendMode::Opaque => BlendMode::Opaque,
        FFIBlendMode::Alpha => BlendMode::Alpha,
    };

    if !surface.pixels.is_null() {
        surf.pixels = Some(crate::display_server::compositor::SurfacePixels {
            ptr: surface.pixels,
            stride: surface.stride,
        });
    } else {
        surf.pixels = None;
    }

    FFIError::Success
}

/// Move surface to new position
#[no_mangle]
pub extern "C" fn staros_display_move_surface(
    handle: usize,
    x: u32,
    y: u32,
) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    let surf = match ds.surface_mut(handle) {
        Some(s) => s,
        None => return FFIError::NotFound,
    };

    surf.x = x;
    surf.y = y;
    FFIError::Success
}

/// Resize surface
#[no_mangle]
pub extern "C" fn staros_display_resize_surface(
    handle: usize,
    width: u32,
    height: u32,
) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    let surf = match ds.surface_mut(handle) {
        Some(s) => s,
        None => return FFIError::NotFound,
    };

    surf.width = width;
    surf.height = height;
    FFIError::Success
}

/// Set surface visibility
#[no_mangle]
pub extern "C" fn staros_display_set_surface_visible(
    handle: usize,
    visible: bool,
) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    let surf = match ds.surface_mut(handle) {
        Some(s) => s,
        None => return FFIError::NotFound,
    };

    surf.visible = visible;
    FFIError::Success
}

/// Set surface z-order
#[no_mangle]
pub extern "C" fn staros_display_set_surface_z_order(
    handle: usize,
    z_order: u8,
) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    let surf = match ds.surface_mut(handle) {
        Some(s) => s,
        None => return FFIError::NotFound,
    };

    surf.z_order = z_order;
    FFIError::Success
}

/// Set background color
#[no_mangle]
pub extern "C" fn staros_display_set_background(color: u32) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    ds.set_background(color);
    FFIError::Success
}

// ---------------------------------------------------------------------------
// Text surface management
// ---------------------------------------------------------------------------

/// Add a text surface
///
/// # Safety
/// - `text` must point to valid UTF-8 data
/// - `text` must remain valid for the lifetime of the text surface
#[no_mangle]
pub unsafe extern "C" fn staros_display_add_text_surface(
    ts: FFITextSurface,
) -> FFIResult<usize> {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let text_bytes = slice::from_raw_parts(ts.text, ts.text_len);
    let text_str = match core::str::from_utf8(text_bytes) {
        Ok(s) => s,
        Err(_) => return FFIResult::err(FFIError::InvalidParameter),
    };

    let text_surface = TextSurface::new(
        ts.x,
        ts.y,
        text_str,
        ts.fg_color,
        Some(ts.bg_color),
    );

    match ds.add_text_surface(text_surface) {
        Some(handle) => FFIResult::ok(handle),
        None => FFIResult::err(FFIError::ResourceExhausted),
    }
}

/// Remove a text surface
#[no_mangle]
pub extern "C" fn staros_display_remove_text_surface(handle: usize) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    ds.remove_text_surface(handle);
    FFIError::Success
}

/// Update text surface content
///
/// # Safety
/// - `text` must point to valid UTF-8 data
#[no_mangle]
pub unsafe extern "C" fn staros_display_update_text_surface(
    handle: usize,
    text: *const u8,
    text_len: usize,
) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    let ts = match ds.text_surface_mut(handle) {
        Some(t) => t,
        None => return FFIError::NotFound,
    };

    let text_bytes = slice::from_raw_parts(text, text_len);
    let text_str = match core::str::from_utf8(text_bytes) {
        Ok(s) => s,
        Err(_) => return FFIError::InvalidParameter,
    };

    ts.set_text(text_str);
    FFIError::Success
}

// ---------------------------------------------------------------------------
// Rendering — synchronous
// ---------------------------------------------------------------------------

/// Present frame (synchronous CPU copy)
///
/// # Safety
/// No other code must be writing to framebuffer concurrently
#[no_mangle]
pub unsafe extern "C" fn staros_display_present() -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    ds.present();
    FFIError::Success
}

/// Clear screen to color (synchronous)
///
/// # Safety
/// No other code must be writing to framebuffer concurrently
#[no_mangle]
pub unsafe extern "C" fn staros_display_clear(color: u32) -> FFIError {
    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    ds.clear(color);
    FFIError::Success
}

// ---------------------------------------------------------------------------
// Rendering — asynchronous (DMA)
// ---------------------------------------------------------------------------

/// Present frame (asynchronous DMA)
///
/// # Safety
/// - No other code must be writing to framebuffer concurrently
/// - `ops`, `chan`, `tracker` must be valid pointers
#[no_mangle]
pub unsafe extern "C" fn staros_display_present_async(
    ops: *const dyn DmaDeviceOps,
    chan: *const DmaChannel,
    tracker: *const DmaCookieTracker,
) -> FFIResult<FFIFlipCookie> {
    if ops.is_null() || chan.is_null() || tracker.is_null() {
        return FFIResult::err(FFIError::InvalidParameter);
    }

    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIResult::err(FFIError::NotInitialized),
    };

    let ops_ref = &*ops;
    let chan_ref = &*chan;
    let tracker_ref = &*tracker;

    match ds.present_async(ops_ref, chan_ref, tracker_ref) {
        Ok(cookie) => FFIResult::ok(FFIFlipCookie { value: cookie.0 }),
        Err(_) => FFIResult::err(FFIError::Busy),
    }
}

/// Wait for DMA flip to complete (blocking)
///
/// # Safety
/// - `ops` and `chan` must be valid pointers
#[no_mangle]
pub unsafe extern "C" fn staros_display_wait_flip(
    cookie: FFIFlipCookie,
    ops: *const dyn DmaDeviceOps,
    chan: *const DmaChannel,
) -> FFIError {
    if ops.is_null() || chan.is_null() {
        return FFIError::InvalidParameter;
    }

    let mut guard = match display_server::get() {
        Some(g) => g,
        None => return FFIError::NotInitialized,
    };

    let ds = match guard.as_mut() {
        Some(ds) => ds,
        None => return FFIError::NotInitialized,
    };

    let ops_ref = &*ops;
    let chan_ref = &*chan;
    let flip_cookie = crate::display_server::FlipCookie(cookie.value);

    ds.wait_flip(flip_cookie, ops_ref, chan_ref);
    FFIError::Success
}

/// Check if DMA flip is complete (non-blocking)
///
/// # Safety
/// - `ops` and `chan` must be valid pointers
#[no_mangle]
pub unsafe extern "C" fn staros_display_flip_done(
    cookie: FFIFlipCookie,
    ops: *const dyn DmaDeviceOps,
    chan: *const DmaChannel,
) -> bool {
    if ops.is_null() || chan.is_null() {
        return false;
    }

    let guard = match display_server::get() {
        Some(g) => g,
        None => return false,
    };

    let ds = match guard.as_ref() {
        Some(ds) => ds,
        None => return false,
    };

    let ops_ref = &*ops;
    let chan_ref = &*chan;
    let flip_cookie = crate::display_server::FlipCookie(cookie.value);

    ds.flip_done(flip_cookie, ops_ref, chan_ref)
}

/// Register callback for DMA flip completion
///
/// NOTE: Callback registration not supported through FFI due to closure limitations.
/// Use staros_display_wait_flip or staros_display_flip_done instead.
#[no_mangle]
pub unsafe extern "C" fn staros_display_on_flip_complete(
    _cookie: FFIFlipCookie,
    _tracker: *const DmaCookieTracker,
    _callback: FFIDmaCallback,
    _user_data: *mut core::ffi::c_void,
) -> FFIError {
    // Cannot wrap FFI callback as fn pointer
    FFIError::Success
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Check if DMA is currently busy
#[no_mangle]
pub extern "C" fn staros_display_dma_busy() -> bool {
    let guard = match display_server::get() {
        Some(g) => g,
        None => return false,
    };

    let ds = match guard.as_ref() {
        Some(ds) => ds,
        None => return false,
    };

    ds.dma_busy()
}

/// Create ARGB8888 color from RGB components
#[no_mangle]
pub extern "C" fn staros_display_rgb(r: u8, g: u8, b: u8) -> u32 {
    crate::drivers::display::framebuffer::rgb(r, g, b)
}

/// Create ARGB8888 color with alpha
#[no_mangle]
pub extern "C" fn staros_display_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}
