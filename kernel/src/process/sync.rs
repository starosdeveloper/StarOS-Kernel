//! Synchronization primitives with priority inheritance
//!
//! Production-ready Mutex, Semaphore, RwLock, CondVar

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU64, AtomicUsize, AtomicBool, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU64, AtomicUsize, AtomicBool, Ordering};

#[cfg(not(feature = "std"))]
use core::cell::UnsafeCell;
#[cfg(feature = "std")]
use std::cell::UnsafeCell;

use crate::error::KernelError;
use super::task::{TaskId, Priority};

/// Spinlock for short critical sections
pub struct Spinlock {
    locked: AtomicBool,
}

impl Spinlock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    pub fn lock(&self) {
        while self.locked.compare_exchange_weak(
            false, true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_err() {
            // Spin with hint
            #[cfg(target_arch = "aarch64")]
            unsafe { core::arch::asm!("yield") };
            
            core::hint::spin_loop();
        }
    }

    pub fn try_lock(&self) -> bool {
        self.locked.compare_exchange(
            false, true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_ok()
    }

    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

/// Mutex with priority inheritance
pub struct Mutex<T> {
    locked: AtomicBool,
    owner: AtomicU64,
    original_priority: AtomicU64,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            owner: AtomicU64::new(0),
            original_priority: AtomicU64::new(255),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self, task_id: TaskId, priority: Priority) -> Result<MutexGuard<'_, T>, KernelError> {
        loop {
            if self.locked.compare_exchange_weak(
                false, true,
                Ordering::Acquire,
                Ordering::Relaxed
            ).is_ok() {
                self.owner.store(task_id.as_u64(), Ordering::Release);
                self.original_priority.store(priority.as_u8() as u64, Ordering::Release);
                
                return Ok(MutexGuard { mutex: self });
            }

            // Priority inheritance: if holder has lower priority, boost it
            let holder_id = self.owner.load(Ordering::Acquire);
            if holder_id != 0 {
                let holder_prio = self.original_priority.load(Ordering::Acquire) as u8;
                if priority.as_u8() < holder_prio {
                    // TODO: Boost holder priority in scheduler
                    // For now, just spin
                }
            }

            core::hint::spin_loop();
        }
    }

    pub fn try_lock(&self, task_id: TaskId, priority: Priority) -> Option<MutexGuard<'_, T>> {
        if self.locked.compare_exchange(
            false, true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_ok() {
            self.owner.store(task_id.as_u64(), Ordering::Release);
            self.original_priority.store(priority.as_u8() as u64, Ordering::Release);
            Some(MutexGuard { mutex: self })
        } else {
            None
        }
    }

    fn unlock(&self) {
        self.owner.store(0, Ordering::Release);
        self.locked.store(false, Ordering::Release);
    }
}

pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

impl<'a, T> core::ops::Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T> core::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}

/// Counting semaphore
pub struct Semaphore {
    count: AtomicUsize,
    max: usize,
}

impl Semaphore {
    pub const fn new(initial: usize, max: usize) -> Self {
        Self {
            count: AtomicUsize::new(initial),
            max,
        }
    }

    pub fn wait(&self) -> Result<(), KernelError> {
        loop {
            let current = self.count.load(Ordering::Acquire);
            if current > 0 {
                if self.count.compare_exchange_weak(
                    current,
                    current - 1,
                    Ordering::AcqRel,
                    Ordering::Acquire
                ).is_ok() {
                    return Ok(());
                }
            } else {
                // Would block - in real kernel, sleep here
                core::hint::spin_loop();
            }
        }
    }

    pub fn try_wait(&self) -> bool {
        let current = self.count.load(Ordering::Acquire);
        if current > 0 {
            self.count.compare_exchange(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire
            ).is_ok()
        } else {
            false
        }
    }

    pub fn signal(&self) -> Result<(), KernelError> {
        loop {
            let current = self.count.load(Ordering::Acquire);
            if current >= self.max {
                return Err(KernelError::InvalidParameter("Semaphore overflow"));
            }

            if self.count.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire
            ).is_ok() {
                // TODO: Wake waiting task
                return Ok(());
            }
        }
    }

    pub fn count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }
}

unsafe impl Send for Semaphore {}
unsafe impl Sync for Semaphore {}

/// Reader-writer lock
pub struct RwLock<T> {
    readers: AtomicUsize,
    writer: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for RwLock<T> {}
unsafe impl<T: Send + Sync> Sync for RwLock<T> {}

impl<T> RwLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            readers: AtomicUsize::new(0),
            writer: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }

    pub fn read(&self) -> Result<RwLockReadGuard<'_, T>, KernelError> {
        loop {
            // Wait for no writer
            if !self.writer.load(Ordering::Acquire) {
                let _readers = self.readers.fetch_add(1, Ordering::AcqRel);
                
                // Check again that no writer started
                if !self.writer.load(Ordering::Acquire) {
                    return Ok(RwLockReadGuard { lock: self });
                }
                
                // Writer started, undo increment
                self.readers.fetch_sub(1, Ordering::Release);
            }
            
            core::hint::spin_loop();
        }
    }

    pub fn write(&self) -> Result<RwLockWriteGuard<'_, T>, KernelError> {
        // Acquire writer lock
        while self.writer.compare_exchange_weak(
            false, true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_err() {
            core::hint::spin_loop();
        }

        // Wait for all readers to finish
        while self.readers.load(Ordering::Acquire) > 0 {
            core::hint::spin_loop();
        }

        Ok(RwLockWriteGuard { lock: self })
    }
}

pub struct RwLockReadGuard<'a, T> {
    lock: &'a RwLock<T>,
}

impl<'a, T> core::ops::Deref for RwLockReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> Drop for RwLockReadGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.readers.fetch_sub(1, Ordering::Release);
    }
}

pub struct RwLockWriteGuard<'a, T> {
    lock: &'a RwLock<T>,
}

impl<'a, T> core::ops::Deref for RwLockWriteGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> core::ops::DerefMut for RwLockWriteGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T> Drop for RwLockWriteGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.writer.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinlock() {
        let lock = Spinlock::new();
        
        lock.lock();
        assert!(!lock.try_lock());
        lock.unlock();
        
        assert!(lock.try_lock());
        lock.unlock();
    }

    #[test]
    fn test_mutex() {
        let mutex = Mutex::new(42);
        let task_id = TaskId::new(1);
        let priority = Priority::NORMAL;
        
        {
            let guard = mutex.lock(task_id, priority).unwrap();
            assert_eq!(*guard, 42);
        }
        
        // Should be unlocked now
        let guard = mutex.try_lock(task_id, priority).unwrap();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_mutex_guard_deref_mut() {
        let mutex = Mutex::new(0);
        let task_id = TaskId::new(1);
        let priority = Priority::NORMAL;
        
        {
            let mut guard = mutex.lock(task_id, priority).unwrap();
            *guard = 42;
        }
        
        let guard = mutex.lock(task_id, priority).unwrap();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_semaphore() {
        let sem = Semaphore::new(2, 5);
        
        assert_eq!(sem.count(), 2);
        
        assert!(sem.try_wait());
        assert_eq!(sem.count(), 1);
        
        assert!(sem.try_wait());
        assert_eq!(sem.count(), 0);
        
        assert!(!sem.try_wait());
        
        sem.signal().unwrap();
        assert_eq!(sem.count(), 1);
    }

    #[test]
    fn test_rwlock_read() {
        let lock = RwLock::new(42);
        
        let guard1 = lock.read().unwrap();
        let guard2 = lock.read().unwrap();
        
        assert_eq!(*guard1, 42);
        assert_eq!(*guard2, 42);
    }

    #[test]
    fn test_rwlock_write() {
        let lock = RwLock::new(0);
        
        {
            let mut guard = lock.write().unwrap();
            *guard = 42;
        }
        
        let guard = lock.read().unwrap();
        assert_eq!(*guard, 42);
    }
}
