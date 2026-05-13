//! FFI type definitions for Kotlin/Native interop
//!
//! All types use `#[repr(C)]` for stable ABI across language boundaries.

use core::ffi::c_void;

// ---------------------------------------------------------------------------
// Display types
// ---------------------------------------------------------------------------

/// C-compatible surface structure for Kotlin/Native
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFISurface {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub color: u32,
    /// Pointer to pixel buffer (ARGB8888), null if solid color
    pub pixels: *const u32,
    /// Stride of pixel buffer in pixels
    pub stride: u32,
    pub blend_mode: FFIBlendMode,
    pub z_order: u8,
    pub visible: bool,
}

/// Blend mode for surfaces
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FFIBlendMode {
    Opaque = 0,
    Alpha = 1,
}

/// Text surface configuration
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFITextSurface {
    pub x: u32,
    pub y: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub fg_color: u32,
    pub bg_color: u32,
    pub font_scale: u32,
    pub z_order: u8,
    pub visible: bool,
    /// Pointer to UTF-8 text buffer
    pub text: *const u8,
    /// Length of text in bytes
    pub text_len: usize,
}

/// Display dimensions
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIDisplayInfo {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub fb_phys: u64,
    pub dma_busy: bool,
}

/// DMA flip cookie for async operations
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIFlipCookie {
    pub value: u32,
}

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Input event type discriminator
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FFIInputEventType {
    MouseMove = 0,
    MouseButton = 1,
    MouseWheel = 2,
    KeyDown = 3,
    KeyUp = 4,
    Touch = 5,
}

/// Mouse button bitmask
pub mod ffi_btn {
    pub const LEFT: u8 = 1 << 0;
    pub const RIGHT: u8 = 1 << 1;
    pub const MIDDLE: u8 = 1 << 2;
}

/// C-compatible input event (tagged union)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FFIInputEvent {
    pub event_type: FFIInputEventType,
    pub data: FFIInputEventData,
}

/// Event data union
#[repr(C)]
#[derive(Clone, Copy)]
pub union FFIInputEventData {
    pub mouse_move: FFIMouseMoveData,
    pub mouse_button: FFIMouseButtonData,
    pub mouse_wheel: FFIMouseWheelData,
    pub key: FFIKeyData,
    pub touch: FFITouchData,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIMouseMoveData {
    pub dx: i16,
    pub dy: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIMouseButtonData {
    pub buttons: u8,
    pub pressed: bool,
    pub x: u32,
    pub y: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIMouseWheelData {
    pub delta: i8,
    pub x: u32,
    pub y: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIKeyData {
    pub scancode: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFITouchData {
    pub id: u8,
    pub x: u32,
    pub y: u32,
    pub pressed: bool,
}

/// Cursor configuration
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFICursor {
    pub x: u32,
    pub y: u32,
    pub visible: bool,
    pub color: u32,
    pub size: u32,
}

impl Default for FFICursor {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            visible: true,
            color: 0xFFFFFFFF,
            size: 16,
        }
    }
}

// ---------------------------------------------------------------------------
// IPC types
// ---------------------------------------------------------------------------

/// IPC message structure
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIMessage {
    pub sender: u64,
    pub msg_type: u64,
    pub data: [u64; 6],
}

/// Signal types
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FFISignal {
    Kill = 1,
    Interrupt = 2,
    Terminate = 3,
    Stop = 4,
    Continue = 5,
    Child = 6,
    User1 = 7,
    User2 = 8,
}

/// Shared memory descriptor
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFISharedMemory {
    pub base: u64,
    pub size: usize,
    pub ref_count: usize,
}

// ---------------------------------------------------------------------------
// Memory types
// ---------------------------------------------------------------------------

/// Memory allocation info
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIMemoryInfo {
    pub total_bytes: usize,
    pub used_bytes: usize,
    pub free_bytes: usize,
    pub largest_free_block: usize,
}

/// Buffer descriptor for zero-copy sharing
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIBuffer {
    pub phys_addr: u64,
    pub virt_addr: u64,
    pub size: usize,
    pub flags: u32,
}

/// Buffer flags
pub mod ffi_buffer_flags {
    pub const READABLE: u32 = 1 << 0;
    pub const WRITABLE: u32 = 1 << 1;
    pub const CACHEABLE: u32 = 1 << 2;
    pub const DMA_CAPABLE: u32 = 1 << 3;
}

// ---------------------------------------------------------------------------
// System types
// ---------------------------------------------------------------------------

/// Process/task information
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFITaskInfo {
    pub task_id: u64,
    pub priority: u8,
    pub state: FFITaskState,
    pub cpu_time_us: u64,
}

/// Task state
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FFITaskState {
    Ready = 0,
    Running = 1,
    Blocked = 2,
    Sleeping = 3,
    Zombie = 4,
}

/// Power state
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FFIPowerState {
    Active = 0,
    Idle = 1,
    Suspend = 2,
    Hibernate = 3,
    Shutdown = 4,
}

/// System event types
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FFISystemEvent {
    PowerButton = 0,
    VolumeUp = 1,
    VolumeDown = 2,
    LowBattery = 3,
    Thermal = 4,
    Watchdog = 5,
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// FFI error codes
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FFIError {
    Success = 0,
    InvalidParameter = 1,
    NotInitialized = 2,
    AlreadyInitialized = 3,
    OutOfMemory = 4,
    ResourceExhausted = 5,
    NotFound = 6,
    PermissionDenied = 7,
    Busy = 8,
    Timeout = 9,
    IoError = 10,
    Unknown = 255,
}

/// Result type for FFI functions
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FFIResult<T: Copy> {
    pub error: FFIError,
    pub value: T,
}

impl<T: Copy + Default> FFIResult<T> {
    pub const fn ok(value: T) -> Self {
        Self {
            error: FFIError::Success,
            value,
        }
    }

    pub fn err(error: FFIError) -> Self {
        Self {
            error,
            value: T::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Callback types
// ---------------------------------------------------------------------------

/// Event callback for Kotlin/Native
pub type FFIEventCallback = extern "C" fn(
    surface_handle: usize,
    event: FFIInputEvent,
    user_data: *mut c_void,
);

/// DMA completion callback
pub type FFIDmaCallback = extern "C" fn(
    cookie: FFIFlipCookie,
    user_data: *mut c_void,
);

/// System event callback
pub type FFISystemEventCallback = extern "C" fn(
    event: FFISystemEvent,
    user_data: *mut c_void,
);

// ---------------------------------------------------------------------------
// Conversion helpers (internal use)
// ---------------------------------------------------------------------------

impl FFIError {
    pub fn from_display_error(err: crate::display_server::DisplayError) -> Self {
        match err {
            crate::display_server::DisplayError::AlreadyInitialised => Self::AlreadyInitialized,
            crate::display_server::DisplayError::BufferTooSmall => Self::OutOfMemory,
        }
    }

    pub fn from_kernel_error(err: crate::error::KernelError) -> Self {
        match err {
            crate::error::KernelError::InvalidParameter(_) => Self::InvalidParameter,
            crate::error::KernelError::NotFound => Self::NotFound,
            _ => Self::Unknown,
        }
    }
}

impl Default for FFIFlipCookie {
    fn default() -> Self {
        Self { value: 0 }
    }
}

impl Default for FFIDisplayInfo {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            stride: 0,
            fb_phys: 0,
            dma_busy: false,
        }
    }
}

impl Default for FFITaskInfo {
    fn default() -> Self {
        Self {
            task_id: 0,
            priority: 0,
            state: FFITaskState::Ready,
            cpu_time_us: 0,
        }
    }
}

impl Default for FFIBuffer {
    fn default() -> Self {
        Self {
            phys_addr: 0,
            virt_addr: 0,
            size: 0,
            flags: 0,
        }
    }
}

impl Default for FFIMemoryInfo {
    fn default() -> Self {
        Self {
            total_bytes: 0,
            used_bytes: 0,
            free_bytes: 0,
            largest_free_block: 0,
        }
    }
}

impl Default for FFIMessage {
    fn default() -> Self {
        Self {
            sender: 0,
            msg_type: 0,
            data: [0; 6],
        }
    }
}

impl Default for FFISharedMemory {
    fn default() -> Self {
        Self {
            base: 0,
            size: 0,
            ref_count: 0,
        }
    }
}

impl Default for FFISignal {
    fn default() -> Self {
        Self::Kill
    }
}
