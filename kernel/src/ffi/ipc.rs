//! IPC FFI — Kotlin/Native bindings for inter-process communication
//!
//! Exports IPC functionality with C ABI:
//! - Message passing (send/receive)
//! - Signal handling
//! - Shared memory management
//! - Event channels

use super::types::*;
use crate::process::ipc::{Message, MessageQueue, Signal, SignalManager, SharedMemory};
use crate::process::task::TaskId;

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

// ---------------------------------------------------------------------------
// Message passing
// ---------------------------------------------------------------------------

/// Send a message to a task
///
/// # Safety
/// - `queue` must be a valid pointer to a MessageQueue
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_send_message(
    queue: *mut MessageQueue,
    msg: FFIMessage,
) -> FFIError {
    if queue.is_null() {
        return FFIError::InvalidParameter;
    }

    let queue_ref = &mut *queue;
    let internal_msg = Message {
        sender: TaskId::new(msg.sender),
        msg_type: msg.msg_type,
        data: msg.data,
    };

    match queue_ref.send(internal_msg) {
        Ok(()) => FFIError::Success,
        Err(e) => FFIError::from_kernel_error(e),
    }
}

/// Receive a message from a task's queue
///
/// Returns the message if available, or error if queue is empty.
///
/// # Safety
/// - `queue` must be a valid pointer to a MessageQueue
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_receive_message(
    queue: *mut MessageQueue,
) -> FFIResult<FFIMessage> {
    if queue.is_null() {
        return FFIResult::err(FFIError::InvalidParameter);
    }

    let queue_ref = &mut *queue;
    match queue_ref.receive() {
        Some(msg) => {
            let ffi_msg = FFIMessage {
                sender: msg.sender.as_u64(),
                msg_type: msg.msg_type,
                data: msg.data,
            };
            FFIResult::ok(ffi_msg)
        }
        None => FFIResult::err(FFIError::NotFound),
    }
}

/// Check if message queue has pending messages
///
/// # Safety
/// - `queue` must be a valid pointer to a MessageQueue
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_has_messages(queue: *const MessageQueue) -> bool {
    if queue.is_null() {
        return false;
    }

    let queue_ref = &*queue;
    !queue_ref.is_empty()
}

/// Get number of pending messages in queue
///
/// # Safety
/// - `queue` must be a valid pointer to a MessageQueue
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_message_count(queue: *const MessageQueue) -> usize {
    if queue.is_null() {
        return 0;
    }

    let queue_ref = &*queue;
    queue_ref.len()
}

/// Check if message queue is full
///
/// # Safety
/// - `queue` must be a valid pointer to a MessageQueue
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_queue_full(queue: *const MessageQueue) -> bool {
    if queue.is_null() {
        return true;
    }

    let queue_ref = &*queue;
    queue_ref.is_full()
}

/// Create a new message queue
///
/// Returns a pointer to the newly created queue, or null on failure.
/// The caller is responsible for freeing the queue with `staros_ipc_destroy_queue`.
#[no_mangle]
pub extern "C" fn staros_ipc_create_queue() -> *mut MessageQueue {
    let queue = Box::new(MessageQueue::new());
    Box::into_raw(queue)
}

/// Destroy a message queue
///
/// # Safety
/// - `queue` must be a valid pointer returned from `staros_ipc_create_queue`
/// - `queue` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_destroy_queue(queue: *mut MessageQueue) {
    if !queue.is_null() {
        let _ = Box::from_raw(queue);
    }
}

// ---------------------------------------------------------------------------
// Signal handling
// ---------------------------------------------------------------------------

/// Send a signal to a task
///
/// # Safety
/// - `signal_mgr` must be a valid pointer to a SignalManager
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_send_signal(
    signal_mgr: *const SignalManager,
    signal: FFISignal,
) -> FFIError {
    if signal_mgr.is_null() {
        return FFIError::InvalidParameter;
    }

    let mgr_ref = &*signal_mgr;
    let internal_signal = match signal {
        FFISignal::Kill => Signal::Kill,
        FFISignal::Interrupt => Signal::Interrupt,
        FFISignal::Terminate => Signal::Terminate,
        FFISignal::Stop => Signal::Stop,
        FFISignal::Continue => Signal::Continue,
        FFISignal::Child => Signal::Child,
        FFISignal::User1 => Signal::User1,
        FFISignal::User2 => Signal::User2,
    };

    mgr_ref.send_signal(internal_signal);
    FFIError::Success
}

/// Check if there are pending signals
///
/// # Safety
/// - `signal_mgr` must be a valid pointer to a SignalManager
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_has_pending_signal(
    signal_mgr: *const SignalManager,
) -> bool {
    if signal_mgr.is_null() {
        return false;
    }

    let mgr_ref = &*signal_mgr;
    mgr_ref.has_pending()
}

/// Get the next pending signal
///
/// Returns the signal if available, or error if no pending signals.
///
/// # Safety
/// - `signal_mgr` must be a valid pointer to a SignalManager
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_get_pending_signal(
    signal_mgr: *const SignalManager,
) -> FFIResult<FFISignal> {
    if signal_mgr.is_null() {
        return FFIResult::err(FFIError::InvalidParameter);
    }

    let mgr_ref = &*signal_mgr;
    match mgr_ref.get_pending() {
        Some(sig) => {
            let ffi_signal = match sig {
                Signal::Kill => FFISignal::Kill,
                Signal::Interrupt => FFISignal::Interrupt,
                Signal::Terminate => FFISignal::Terminate,
                Signal::Stop => FFISignal::Stop,
                Signal::Continue => FFISignal::Continue,
                Signal::Child => FFISignal::Child,
                Signal::User1 => FFISignal::User1,
                Signal::User2 => FFISignal::User2,
            };
            FFIResult::ok(ffi_signal)
        }
        None => FFIResult::err(FFIError::NotFound),
    }
}

/// Clear a specific signal
///
/// # Safety
/// - `signal_mgr` must be a valid pointer to a SignalManager
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_clear_signal(
    signal_mgr: *const SignalManager,
    signal: FFISignal,
) -> FFIError {
    if signal_mgr.is_null() {
        return FFIError::InvalidParameter;
    }

    let mgr_ref = &*signal_mgr;
    let internal_signal = match signal {
        FFISignal::Kill => Signal::Kill,
        FFISignal::Interrupt => Signal::Interrupt,
        FFISignal::Terminate => Signal::Terminate,
        FFISignal::Stop => Signal::Stop,
        FFISignal::Continue => Signal::Continue,
        FFISignal::Child => Signal::Child,
        FFISignal::User1 => Signal::User1,
        FFISignal::User2 => Signal::User2,
    };

    mgr_ref.clear_signal(internal_signal);
    FFIError::Success
}

/// Block a signal
///
/// # Safety
/// - `signal_mgr` must be a valid pointer to a SignalManager
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_block_signal(
    signal_mgr: *const SignalManager,
    signal: FFISignal,
) -> FFIError {
    if signal_mgr.is_null() {
        return FFIError::InvalidParameter;
    }

    let mgr_ref = &*signal_mgr;
    let internal_signal = match signal {
        FFISignal::Kill => Signal::Kill,
        FFISignal::Interrupt => Signal::Interrupt,
        FFISignal::Terminate => Signal::Terminate,
        FFISignal::Stop => Signal::Stop,
        FFISignal::Continue => Signal::Continue,
        FFISignal::Child => Signal::Child,
        FFISignal::User1 => Signal::User1,
        FFISignal::User2 => Signal::User2,
    };

    mgr_ref.block_signal(internal_signal);
    FFIError::Success
}

/// Unblock a signal
///
/// # Safety
/// - `signal_mgr` must be a valid pointer to a SignalManager
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_unblock_signal(
    signal_mgr: *const SignalManager,
    signal: FFISignal,
) -> FFIError {
    if signal_mgr.is_null() {
        return FFIError::InvalidParameter;
    }

    let mgr_ref = &*signal_mgr;
    let internal_signal = match signal {
        FFISignal::Kill => Signal::Kill,
        FFISignal::Interrupt => Signal::Interrupt,
        FFISignal::Terminate => Signal::Terminate,
        FFISignal::Stop => Signal::Stop,
        FFISignal::Continue => Signal::Continue,
        FFISignal::Child => Signal::Child,
        FFISignal::User1 => Signal::User1,
        FFISignal::User2 => Signal::User2,
    };

    mgr_ref.unblock_signal(internal_signal);
    FFIError::Success
}

/// Create a new signal manager
#[no_mangle]
pub extern "C" fn staros_ipc_create_signal_manager() -> *mut SignalManager {
    let mgr = Box::new(SignalManager::new());
    Box::into_raw(mgr)
}

/// Destroy a signal manager
///
/// # Safety
/// - `signal_mgr` must be a valid pointer returned from `staros_ipc_create_signal_manager`
/// - `signal_mgr` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_destroy_signal_manager(signal_mgr: *mut SignalManager) {
    if !signal_mgr.is_null() {
        let _ = Box::from_raw(signal_mgr);
    }
}

// ---------------------------------------------------------------------------
// Shared memory
// ---------------------------------------------------------------------------

/// Create a shared memory region
///
/// Returns a pointer to the shared memory descriptor, or null on failure.
#[no_mangle]
pub extern "C" fn staros_ipc_create_shared_memory(
    base: u64,
    size: usize,
) -> *mut SharedMemory {
    let shm = match SharedMemory::new(base, size) {
        Ok(s) => Box::new(s),
        Err(_) => return core::ptr::null_mut(),
    };
    Box::into_raw(shm)
}

/// Destroy a shared memory region
///
/// # Safety
/// - `shm` must be a valid pointer returned from `staros_ipc_create_shared_memory`
/// - `shm` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_destroy_shared_memory(shm: *mut SharedMemory) {
    if !shm.is_null() {
        let _ = Box::from_raw(shm);
    }
}

/// Get shared memory info
///
/// # Safety
/// - `shm` must be a valid pointer to a SharedMemory
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_get_shared_memory_info(
    shm: *const SharedMemory,
) -> FFIResult<FFISharedMemory> {
    if shm.is_null() {
        return FFIResult::err(FFIError::InvalidParameter);
    }

    let shm_ref = &*shm;
    let info = FFISharedMemory {
        base: shm_ref.base(),
        size: shm_ref.size(),
        ref_count: shm_ref.ref_count(),
    };

    FFIResult::ok(info)
}

/// Attach to a shared memory region (increment reference count)
///
/// # Safety
/// - `shm` must be a valid pointer to a SharedMemory
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_attach_shared_memory(
    shm: *const SharedMemory,
) -> FFIError {
    if shm.is_null() {
        return FFIError::InvalidParameter;
    }

    let shm_ref = &*shm;
    match shm_ref.attach() {
        Ok(()) => FFIError::Success,
        Err(e) => FFIError::from_kernel_error(e),
    }
}

/// Detach from a shared memory region (decrement reference count)
///
/// Returns true if this was the last reference (region should be freed).
///
/// # Safety
/// - `shm` must be a valid pointer to a SharedMemory
#[no_mangle]
pub unsafe extern "C" fn staros_ipc_detach_shared_memory(
    shm: *const SharedMemory,
    last_ref: *mut bool,
) -> FFIError {
    if shm.is_null() {
        return FFIError::InvalidParameter;
    }

    let shm_ref = &*shm;
    match shm_ref.detach() {
        Ok(is_last) => {
            if !last_ref.is_null() {
                *last_ref = is_last;
            }
            FFIError::Success
        }
        Err(e) => FFIError::from_kernel_error(e),
    }
}

// ---------------------------------------------------------------------------
// Utility functions
// ---------------------------------------------------------------------------

/// Create a message with data
#[no_mangle]
pub extern "C" fn staros_ipc_create_message(
    sender: u64,
    msg_type: u64,
    data: *const u64,
    data_len: usize,
) -> FFIMessage {
    let mut msg_data = [0u64; 6];
    
    if !data.is_null() && data_len > 0 {
        let copy_len = data_len.min(6);
        unsafe {
            let data_slice = core::slice::from_raw_parts(data, copy_len);
            msg_data[..copy_len].copy_from_slice(data_slice);
        }
    }

    FFIMessage {
        sender,
        msg_type,
        data: msg_data,
    }
}

/// Get message data at specific index
#[no_mangle]
pub extern "C" fn staros_ipc_get_message_data(msg: FFIMessage, index: usize) -> u64 {
    if index < 6 {
        msg.data[index]
    } else {
        0
    }
}

/// Set message data at specific index
#[no_mangle]
pub extern "C" fn staros_ipc_set_message_data(
    msg: *mut FFIMessage,
    index: usize,
    value: u64,
) -> FFIError {
    if msg.is_null() || index >= 6 {
        return FFIError::InvalidParameter;
    }

    unsafe {
        (*msg).data[index] = value;
    }

    FFIError::Success
}
