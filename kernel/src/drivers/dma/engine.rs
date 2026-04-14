// SPDX-License-Identifier: MIT OR Apache-2.0
//! DMA Engine Core
//!
//! Ported from Linux: drivers/dma/dmaengine.c
//! Source lines: ~1400 C → ~700 Rust

use crate::drivers::base::Device;
use spin::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// DMA transfer direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaDirection {
    MemToMem = 0,
    MemToDev = 1,
    DevToMem = 2,
    DevToDev = 3,
}

/// DMA transfer status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaStatus {
    Complete = 0,
    InProgress = 1,
    Paused = 2,
    Error = 3,
}

/// DMA capabilities
pub mod dma_caps {
    pub const MEMCPY: u32 = 1 << 0;
    pub const XOR: u32 = 1 << 1;
    pub const PQ: u32 = 1 << 2;
    pub const INTERRUPT: u32 = 1 << 3;
    pub const CYCLIC: u32 = 1 << 4;
    pub const INTERLEAVE: u32 = 1 << 5;
    pub const PRIVATE: u32 = 1 << 6;
}

/// DMA channel
#[derive(Debug)]
pub struct DmaChannel {
    pub chan_id: u32,
    pub device: DmaDevice,
    pub client_count: AtomicU32,
    pub name: &'static str,
}

impl DmaChannel {
    pub fn new(chan_id: u32, device: DmaDevice, name: &'static str) -> Self {
        Self {
            chan_id,
            device,
            client_count: AtomicU32::new(0),
            name,
        }
    }
}

/// DMA completion callback type
pub type DmaCallback = fn(cookie: u32);

/// DMA device operations
pub trait DmaDeviceOps {
    fn device_prep_dma_memcpy(&self, dest: u64, src: u64, len: usize) -> Result<DmaDescriptor, i32>;
    fn device_prep_slave_sg(&self, sgl: &[ScatterGatherEntry], direction: DmaDirection) -> Result<DmaDescriptor, i32>;
    fn device_issue_pending(&self, chan: &DmaChannel);
    fn device_tx_status(&self, chan: &DmaChannel, cookie: u32) -> DmaStatus;
    fn device_terminate_all(&self, chan: &DmaChannel) -> Result<(), i32>;
    fn device_pause(&self, chan: &DmaChannel) -> Result<(), i32>;
    fn device_resume(&self, chan: &DmaChannel) -> Result<(), i32>;
    fn device_callback_request(&self, chan: &DmaChannel, callback: DmaCallback);
}

/// Scatter-Gather entry
#[derive(Debug, Clone)]
pub struct ScatterGatherEntry {
    pub addr: u64,
    pub len: usize,
}

/// DMA descriptor queue (ring buffer)
pub struct DmaDescriptorQueue {
    descriptors: Vec<DmaDescriptor>,
    head: AtomicUsize,
    tail: AtomicUsize,
    capacity: usize,
}

impl DmaDescriptorQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            descriptors: Vec::with_capacity(capacity),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            capacity,
        }
    }

    pub fn enqueue(&mut self, desc: DmaDescriptor) -> Result<(), i32> {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        
        if (tail + 1) % self.capacity == head {
            return Err(-28); // -ENOSPC
        }
        
        if tail < self.descriptors.len() {
            self.descriptors[tail] = desc;
        } else {
            self.descriptors.push(desc);
        }
        
        self.tail.store((tail + 1) % self.capacity, Ordering::Release);
        Ok(())
    }

    pub fn dequeue(&mut self) -> Option<DmaDescriptor> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        
        if head == tail {
            return None;
        }
        
        let desc = self.descriptors[head].clone();
        self.head.store((head + 1) % self.capacity, Ordering::Release);
        Some(desc)
    }

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }
}

/// Cookie tracker for DMA transactions
pub struct DmaCookieTracker {
    next_cookie: AtomicU32,
    completed: Mutex<Vec<u32>>,
    callbacks: Mutex<Vec<(u32, DmaCallback)>>,
}

impl DmaCookieTracker {
    pub fn new() -> Self {
        Self {
            next_cookie: AtomicU32::new(1),
            completed: Mutex::new(Vec::new()),
            callbacks: Mutex::new(Vec::new()),
        }
    }

    pub fn assign_cookie(&self) -> u32 {
        self.next_cookie.fetch_add(1, Ordering::Relaxed)
    }

    pub fn mark_complete(&self, cookie: u32) {
        self.completed.lock().push(cookie);
        
        // Call registered callback
        let mut callbacks = self.callbacks.lock();
        if let Some(pos) = callbacks.iter().position(|(c, _)| *c == cookie) {
            let (_, callback) = callbacks.remove(pos);
            callback(cookie);
        }
    }

    pub fn register_callback(&self, cookie: u32, callback: DmaCallback) {
        self.callbacks.lock().push((cookie, callback));
    }

    pub fn is_complete(&self, cookie: u32) -> bool {
        self.completed.lock().contains(&cookie)
    }

    pub fn clear_completed(&self) {
        self.completed.lock().clear();
    }
}

/// DMA IRQ handler - called from interrupt context
///
/// This should be called by DMA controller driver when transfer completes
pub fn dma_irq_handler(chan: &DmaChannel, cookie: u32, tracker: &DmaCookieTracker) {
    // Mark transaction as complete
    tracker.mark_complete(cookie);
    
    // Update channel status
    // In full implementation, would update channel state
}

/// DMA device
#[derive(Debug)]
pub struct DmaDevice {
    pub dev_id: u32,
    pub dev: Device,
    pub cap_mask: u32,
    pub chancnt: u32,
    pub privatecnt: AtomicU32,
}

impl DmaDevice {
    pub fn new(dev: Device) -> Self {
        Self {
            dev_id: 0,
            dev,
            cap_mask: 0,
            chancnt: 0,
            privatecnt: AtomicU32::new(0),
        }
    }

    pub fn has_cap(&self, cap: u32) -> bool {
        (self.cap_mask & cap) != 0
    }
}

/// DMA descriptor
#[derive(Debug, Clone)]
pub struct DmaDescriptor {
    pub cookie: u32,
    pub phys: u64,
    pub len: usize,
    pub direction: DmaDirection,
}

/// Global DMA device list
static DMA_DEVICE_LIST: Mutex<Vec<DmaDevice>> = Mutex::new(Vec::new());
static DMA_REF_COUNT: AtomicUsize = AtomicUsize::new(0);
static NEXT_DEV_ID: AtomicU32 = AtomicU32::new(0);

/// DMA channel with queue and cookie tracking
impl DmaChannel {
    pub fn new_with_queue(chan_id: u32, device: DmaDevice, name: &'static str, queue_size: usize) -> Self {
        Self {
            chan_id,
            device,
            client_count: AtomicU32::new(0),
            name,
        }
    }

    pub fn submit_descriptor(&self, mut desc: DmaDescriptor, tracker: &DmaCookieTracker) -> u32 {
        desc.cookie = tracker.assign_cookie();
        // In full implementation, would add to queue here
        desc.cookie
    }
}

/// Register DMA device
///
/// Ported from: dma_async_device_register()
pub fn dma_async_device_register(mut device: DmaDevice) -> Result<(), i32> {
    // Validate device
    if device.chancnt == 0 {
        return Err(-22); // -EINVAL
    }

    // Assign device ID
    device.dev_id = NEXT_DEV_ID.fetch_add(1, Ordering::Relaxed);

    // Add to global list
    let mut list = DMA_DEVICE_LIST.lock();
    list.push(device);

    Ok(())
}

/// Unregister DMA device
///
/// Ported from: dma_async_device_unregister()
pub fn dma_async_device_unregister(device: &DmaDevice) {
    let mut list = DMA_DEVICE_LIST.lock();
    list.retain(|d| d.dev_id != device.dev_id);
}

/// Get DMA engine reference
///
/// Ported from: dmaengine_get()
pub fn dmaengine_get() {
    DMA_REF_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Put DMA engine reference
///
/// Ported from: dmaengine_put()
pub fn dmaengine_put() {
    let count = DMA_REF_COUNT.fetch_sub(1, Ordering::Relaxed);
    if count == 0 {
        panic!("dmaengine_put: ref_count underflow");
    }
}

/// Get channel reference
///
/// Ported from: dma_chan_get()
fn dma_chan_get(chan: &DmaChannel) -> Result<(), i32> {
    chan.client_count.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Put channel reference
///
/// Ported from: dma_chan_put()
fn dma_chan_put(chan: &DmaChannel) {
    let count = chan.client_count.fetch_sub(1, Ordering::Relaxed);
    if count == 0 {
        panic!("dma_chan_put: client_count underflow");
    }
}

/// Find suitable DMA channel
///
/// Ported from: dma_find_channel()
pub fn dma_find_channel(tx_type: u32) -> Option<DmaChannel> {
    let list = DMA_DEVICE_LIST.lock();
    
    for device in list.iter() {
        if device.has_cap(tx_type) && !device.has_cap(dma_caps::PRIVATE) {
            // Return first available channel
            // In full implementation, would iterate device.channels
            return None; // Placeholder
        }
    }
    
    None
}

/// Request exclusive DMA channel
///
/// Ported from: dma_request_channel()
pub fn dma_request_channel(mask: u32) -> Option<DmaChannel> {
    let list = DMA_DEVICE_LIST.lock();
    
    for device in list.iter() {
        if (device.cap_mask & mask) == mask {
            // Return first matching channel
            return None; // Placeholder
        }
    }
    
    None
}

/// Release DMA channel
///
/// Ported from: dma_release_channel()
pub fn dma_release_channel(chan: &DmaChannel) {
    dma_chan_put(chan);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dma_device_new() {
        let dev = Device::mock();
        let dma_dev = DmaDevice::new(dev);
        assert_eq!(dma_dev.chancnt, 0);
        assert_eq!(dma_dev.cap_mask, 0);
    }

    #[test]
    fn test_dma_device_has_cap() {
        let dev = Device::mock();
        let mut dma_dev = DmaDevice::new(dev);
        dma_dev.cap_mask = dma_caps::MEMCPY | dma_caps::INTERRUPT;
        
        assert!(dma_dev.has_cap(dma_caps::MEMCPY));
        assert!(dma_dev.has_cap(dma_caps::INTERRUPT));
        assert!(!dma_dev.has_cap(dma_caps::XOR));
    }

    #[test]
    fn test_dma_async_device_register() {
        let dev = Device::mock();
        let mut dma_dev = DmaDevice::new(dev);
        dma_dev.chancnt = 4;
        dma_dev.cap_mask = dma_caps::MEMCPY;
        
        let result = dma_async_device_register(dma_dev);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dmaengine_get_put() {
        let initial = DMA_REF_COUNT.load(Ordering::Relaxed);
        
        dmaengine_get();
        assert_eq!(DMA_REF_COUNT.load(Ordering::Relaxed), initial + 1);
        
        dmaengine_put();
        assert_eq!(DMA_REF_COUNT.load(Ordering::Relaxed), initial);
    }

    #[test]
    fn test_dma_channel_new() {
        let dev = Device::mock();
        let dma_dev = DmaDevice::new(dev);
        let chan = DmaChannel::new(0, dma_dev, "dma0chan0");
        
        assert_eq!(chan.chan_id, 0);
        assert_eq!(chan.name, "dma0chan0");
        assert_eq!(chan.client_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_dma_chan_get_put() {
        let dev = Device::mock();
        let dma_dev = DmaDevice::new(dev);
        let chan = DmaChannel::new(0, dma_dev, "test");
        
        let result = dma_chan_get(&chan);
        assert!(result.is_ok());
        assert_eq!(chan.client_count.load(Ordering::Relaxed), 1);
        
        dma_chan_put(&chan);
        assert_eq!(chan.client_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_dma_descriptor_queue() {
        let mut queue = DmaDescriptorQueue::new(4);
        
        let desc1 = DmaDescriptor {
            cookie: 1,
            phys: 0x1000,
            len: 1024,
            direction: DmaDirection::MemToMem,
        };
        
        assert!(queue.enqueue(desc1.clone()).is_ok());
        assert!(!queue.is_empty());
        
        let dequeued = queue.dequeue();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().cookie, 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_dma_cookie_tracker() {
        let tracker = DmaCookieTracker::new();
        
        let cookie1 = tracker.assign_cookie();
        let cookie2 = tracker.assign_cookie();
        
        assert_eq!(cookie1, 1);
        assert_eq!(cookie2, 2);
        
        tracker.mark_complete(cookie1);
        assert!(tracker.is_complete(cookie1));
        assert!(!tracker.is_complete(cookie2));
    }

    #[test]
    fn test_scatter_gather_entry() {
        let sg = ScatterGatherEntry {
            addr: 0x1000,
            len: 4096,
        };
        
        assert_eq!(sg.addr, 0x1000);
        assert_eq!(sg.len, 4096);
    }
}
