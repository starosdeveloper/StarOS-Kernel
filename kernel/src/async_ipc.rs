//! Async IPC - Non-blocking message passing with futures/wakers
//!
//! Provides async channels for microkernel IPC without blocking the sender.

use core::task::{Waker, Context, Poll};
use core::pin::Pin;
use core::future::Future;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use crate::error::KernelError;
use crate::process::task::TaskId;

/// Maximum messages in an async channel
const CHANNEL_CAPACITY: usize = 64;

/// IPC message
#[derive(Clone)]
pub struct IpcMessage {
    pub sender: TaskId,
    pub msg_type: u32,
    pub payload: Vec<u8>,
}

/// Async IPC channel
pub struct Channel {
    queue: Mutex<VecDeque<IpcMessage>>,
    wakers: Mutex<Vec<Waker>>,
    closed: AtomicBool,
    msg_count: AtomicU32,
}

impl Channel {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::with_capacity(CHANNEL_CAPACITY)),
            wakers: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
            msg_count: AtomicU32::new(0),
        }
    }

    /// Send a message (non-blocking, returns error if full)
    pub fn send(&self, msg: IpcMessage) -> Result<(), KernelError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(KernelError::OperationFailed);
        }
        let mut queue = self.queue.lock();
        if queue.len() >= CHANNEL_CAPACITY {
            return Err(KernelError::ResourceExhausted);
        }
        queue.push_back(msg);
        self.msg_count.fetch_add(1, Ordering::Relaxed);
        drop(queue);
        // Wake all waiting receivers
        let mut wakers = self.wakers.lock();
        for w in wakers.drain(..) {
            w.wake();
        }
        Ok(())
    }

    /// Try to receive (non-blocking)
    pub fn try_recv(&self) -> Option<IpcMessage> {
        self.queue.lock().pop_front()
    }

    /// Get a future that resolves when a message is available
    pub fn recv(&self) -> RecvFuture<'_> {
        RecvFuture { channel: self }
    }

    /// Close the channel
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let mut wakers = self.wakers.lock();
        for w in wakers.drain(..) {
            w.wake();
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn len(&self) -> usize {
        self.queue.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Future that resolves when a message is available
pub struct RecvFuture<'a> {
    channel: &'a Channel,
}

impl<'a> Future for RecvFuture<'a> {
    type Output = Result<IpcMessage, KernelError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Try to get a message
        if let Some(msg) = self.channel.try_recv() {
            return Poll::Ready(Ok(msg));
        }
        // Channel closed?
        if self.channel.is_closed() {
            return Poll::Ready(Err(KernelError::OperationFailed));
        }
        // Register waker for notification
        self.channel.wakers.lock().push(cx.waker().clone());
        // Double-check (avoid race)
        if let Some(msg) = self.channel.try_recv() {
            return Poll::Ready(Ok(msg));
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_recv() {
        let ch = Channel::new();
        let msg = IpcMessage { sender: TaskId::new(1), msg_type: 42, payload: vec![1,2,3] };
        ch.send(msg).unwrap();
        let received = ch.try_recv().unwrap();
        assert_eq!(received.msg_type, 42);
        assert_eq!(received.payload, vec![1,2,3]);
    }

    #[test]
    fn test_capacity_limit() {
        let ch = Channel::new();
        for i in 0..CHANNEL_CAPACITY {
            ch.send(IpcMessage { sender: TaskId::new(1), msg_type: i as u32, payload: vec![] }).unwrap();
        }
        assert!(ch.send(IpcMessage { sender: TaskId::new(1), msg_type: 0, payload: vec![] }).is_err());
    }

    #[test]
    fn test_close() {
        let ch = Channel::new();
        ch.close();
        assert!(ch.send(IpcMessage { sender: TaskId::new(1), msg_type: 0, payload: vec![] }).is_err());
    }
}
