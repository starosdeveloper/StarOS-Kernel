//! Inter-Process Communication
//!
//! Message passing, signals, and shared memory

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::error::KernelError;
use super::task::TaskId;

/// Message for IPC
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Message {
    pub sender: TaskId,
    pub msg_type: u64,
    pub data: [u64; 6], // 48 bytes of data
}

impl Message {
    pub const fn new(sender: TaskId, msg_type: u64) -> Self {
        Self {
            sender,
            msg_type,
            data: [0; 6],
        }
    }

    pub fn with_data(sender: TaskId, msg_type: u64, data: [u64; 6]) -> Self {
        Self {
            sender,
            msg_type,
            data,
        }
    }
}

/// Message queue for task-to-task communication
pub struct MessageQueue {
    messages: [Option<Message>; 32],
    head: AtomicUsize,
    tail: AtomicUsize,
    count: AtomicUsize,
}

impl MessageQueue {
    /// Maximum message data size in bytes
    pub const MAX_MESSAGE_SIZE: usize = 4096;

    pub const fn new() -> Self {
        Self {
            messages: [None; 32],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }

    pub fn send(&mut self, msg: Message) -> Result<(), KernelError> {
        // Reject messages exceeding maximum data size
        if core::mem::size_of::<Message>() > Self::MAX_MESSAGE_SIZE {
            return Err(KernelError::InvalidParameter("Message exceeds maximum size of 4096 bytes"));
        }

        let count = self.count.load(Ordering::Acquire);
        if count >= 32 {
            return Err(KernelError::ResourceExhausted);
        }

        let tail = self.tail.load(Ordering::Acquire);
        self.messages[tail] = Some(msg);
        
        self.tail.store((tail + 1) % 32, Ordering::Release);
        self.count.fetch_add(1, Ordering::Release);

        Ok(())
    }

    /// Send raw data with size validation (max 4096 bytes)
    pub fn send_data(&mut self, sender: TaskId, msg_type: u64, data: &[u8]) -> Result<(), KernelError> {
        if data.len() > Self::MAX_MESSAGE_SIZE {
            return Err(KernelError::InvalidParameter("Message data exceeds 4096 bytes"));
        }

        let mut msg = Message::new(sender, msg_type);
        let copy_len = data.len().min(core::mem::size_of_val(&msg.data));
        // SAFETY: copying user data into fixed message buffer within bounds
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                msg.data.as_mut_ptr() as *mut u8,
                copy_len,
            );
        }
        self.send(msg)
    }

    pub fn receive(&mut self) -> Option<Message> {
        let count = self.count.load(Ordering::Acquire);
        if count == 0 {
            return None;
        }

        let head = self.head.load(Ordering::Acquire);
        let msg = self.messages[head].take();
        
        self.head.store((head + 1) % 32, Ordering::Release);
        self.count.fetch_sub(1, Ordering::Release);

        msg
    }

    pub fn len(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_full(&self) -> bool {
        self.len() >= 32
    }
}

unsafe impl Send for MessageQueue {}
unsafe impl Sync for MessageQueue {}

/// Signal types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Signal {
    Kill = 1,
    Interrupt = 2,
    Terminate = 3,
    Stop = 4,
    Continue = 5,
    Child = 6,
    User1 = 7,
    User2 = 8,
}

impl Signal {
    fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(Self::Kill),
            2 => Some(Self::Interrupt),
            3 => Some(Self::Terminate),
            4 => Some(Self::Stop),
            5 => Some(Self::Continue),
            6 => Some(Self::Child),
            7 => Some(Self::User1),
            8 => Some(Self::User2),
            _ => None,
        }
    }
}

/// Signal handler
pub type SignalHandler = fn(Signal);

/// Signal management for tasks
pub struct SignalManager {
    pending: AtomicU64, // Bitmap of pending signals
    blocked: AtomicU64, // Bitmap of blocked signals
    handlers: [Option<SignalHandler>; 8],
}

impl SignalManager {
    pub const fn new() -> Self {
        Self {
            pending: AtomicU64::new(0),
            blocked: AtomicU64::new(0),
            handlers: [None; 8],
        }
    }

    pub fn send_signal(&self, signal: Signal) {
        let bit = 1u64 << (signal as u8);
        self.pending.fetch_or(bit, Ordering::Release);
    }

    pub fn has_pending(&self) -> bool {
        let pending = self.pending.load(Ordering::Acquire);
        let blocked = self.blocked.load(Ordering::Acquire);
        (pending & !blocked) != 0
    }

    pub fn get_pending(&self) -> Option<Signal> {
        let pending = self.pending.load(Ordering::Acquire);
        let blocked = self.blocked.load(Ordering::Acquire);
        let deliverable = pending & !blocked;

        if deliverable == 0 {
            return None;
        }

        // Find first set bit (lowest signal number)
        let bit_pos = deliverable.trailing_zeros();
        Signal::from_u8(bit_pos as u8)
    }

    pub fn clear_signal(&self, signal: Signal) {
        let bit = !(1u64 << (signal as u8));
        self.pending.fetch_and(bit, Ordering::Release);
    }

    pub fn block_signal(&self, signal: Signal) {
        let bit = 1u64 << (signal as u8);
        self.blocked.fetch_or(bit, Ordering::Release);
    }

    pub fn unblock_signal(&self, signal: Signal) {
        let bit = !(1u64 << (signal as u8));
        self.blocked.fetch_and(bit, Ordering::Release);
    }

    pub fn set_handler(&mut self, signal: Signal, handler: SignalHandler) {
        let idx = (signal as u8 - 1) as usize;
        if idx < 8 {
            self.handlers[idx] = Some(handler);
        }
    }

    pub fn get_handler(&self, signal: Signal) -> Option<SignalHandler> {
        let idx = (signal as u8 - 1) as usize;
        if idx < 8 {
            self.handlers[idx]
        } else {
            None
        }
    }
}

unsafe impl Send for SignalManager {}
unsafe impl Sync for SignalManager {}

/// Shared memory region
pub struct SharedMemory {
    base: u64,
    size: usize,
    ref_count: AtomicUsize,
}

impl SharedMemory {
    /// Maximum shared memory size (64MB)
    pub const MAX_SIZE: usize = 64 * 1024 * 1024;

    pub fn new(base: u64, size: usize) -> Result<Self, KernelError> {
        if size > Self::MAX_SIZE {
            return Err(KernelError::InvalidParameter("Shared memory exceeds 64MB limit"));
        }
        Ok(Self {
            base,
            size,
            ref_count: AtomicUsize::new(1),
        })
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn ref_count(&self) -> usize {
        self.ref_count.load(Ordering::Acquire)
    }

    pub fn attach(&self) -> Result<(), KernelError> {
        self.ref_count.fetch_add(1, Ordering::Release);
        Ok(())
    }

    pub fn detach(&self) -> Result<bool, KernelError> {
        let old = self.ref_count.fetch_sub(1, Ordering::Release);
        if old == 0 {
            return Err(KernelError::InvalidParameter("Invalid ref count"));
        }
        Ok(old == 1) // Return true if this was last reference
    }
}

unsafe impl Send for SharedMemory {}
unsafe impl Sync for SharedMemory {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message() {
        let msg = Message::new(TaskId::new(1), 42);
        assert_eq!(msg.sender, TaskId::new(1));
        assert_eq!(msg.msg_type, 42);
    }

    #[test]
    fn test_message_queue() {
        let mut queue = MessageQueue::new();
        
        assert!(queue.is_empty());
        
        let msg1 = Message::new(TaskId::new(1), 1);
        let msg2 = Message::new(TaskId::new(2), 2);
        
        queue.send(msg1).unwrap();
        queue.send(msg2).unwrap();
        
        assert_eq!(queue.len(), 2);
        
        let received1 = queue.receive().unwrap();
        assert_eq!(received1.sender, TaskId::new(1));
        
        let received2 = queue.receive().unwrap();
        assert_eq!(received2.sender, TaskId::new(2));
        
        assert!(queue.is_empty());
    }

    #[test]
    fn test_message_queue_full() {
        let mut queue = MessageQueue::new();
        
        for i in 0..32 {
            let msg = Message::new(TaskId::new(i), i);
            queue.send(msg).unwrap();
        }
        
        assert!(queue.is_full());
        
        let msg = Message::new(TaskId::new(99), 99);
        assert!(queue.send(msg).is_err());
    }

    #[test]
    fn test_signal_manager() {
        let mgr = SignalManager::new();
        
        assert!(!mgr.has_pending());
        
        mgr.send_signal(Signal::Interrupt);
        assert!(mgr.has_pending());
        
        let sig = mgr.get_pending().unwrap();
        assert_eq!(sig, Signal::Interrupt);
        
        mgr.clear_signal(Signal::Interrupt);
        assert!(!mgr.has_pending());
    }

    #[test]
    fn test_signal_blocking() {
        let mgr = SignalManager::new();
        
        mgr.block_signal(Signal::Interrupt);
        mgr.send_signal(Signal::Interrupt);
        
        assert!(!mgr.has_pending()); // Blocked
        
        mgr.unblock_signal(Signal::Interrupt);
        assert!(mgr.has_pending()); // Now deliverable
    }

    #[test]
    fn test_shared_memory() {
        let shm = SharedMemory::new(0x10000, 4096).unwrap();
        
        assert_eq!(shm.ref_count(), 1);
        
        shm.attach().unwrap();
        assert_eq!(shm.ref_count(), 2);
        
        assert!(!shm.detach().unwrap()); // Not last ref
        assert!(shm.detach().unwrap());  // Last ref
    }
}
