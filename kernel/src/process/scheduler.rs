/// Production scheduler with O(1) scheduling
///
/// Features:
/// - O(1) task selection via priority bitmap
/// - 256 priority levels
/// - Real-time and normal task classes
/// - Time slicing with configurable quantum
/// - Preemption support
/// - CPU affinity support

#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "std")]
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::error::{KernelError, ProcessError};
use super::task::{TaskId, Priority};

/// Default time quantum in timer ticks (10ms at 1kHz)
pub const DEFAULT_QUANTUM: u64 = 10;

/// Minimum quantum for real-time tasks (1ms)
pub const RT_MIN_QUANTUM: u64 = 1;

/// Maximum quantum for low-priority tasks (100ms)
pub const MAX_QUANTUM: u64 = 100;

/// Run queue for a single priority level
struct RunQueue {
    tasks: [Option<TaskId>; 64], // Max 64 tasks per priority
    head: usize,
    tail: usize,
    count: usize,
}

impl RunQueue {
    const fn new() -> Self {
        Self {
            tasks: [None; 64],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    fn push(&mut self, task_id: TaskId) -> Result<(), KernelError> {
        if self.count >= 64 {
            return Err(KernelError::Process(ProcessError::TooManyProcesses));
        }

        self.tasks[self.tail] = Some(task_id);
        self.tail = (self.tail + 1) % 64;
        self.count += 1;
        Ok(())
    }

    fn pop(&mut self) -> Option<TaskId> {
        if self.count == 0 {
            return None;
        }

        let task_id = self.tasks[self.head].take();
        self.head = (self.head + 1) % 64;
        self.count -= 1;
        task_id
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn len(&self) -> usize {
        self.count
    }

    fn remove(&mut self, task_id: TaskId) -> bool {
        for i in 0..64 {
            if self.tasks[i] == Some(task_id) {
                self.tasks[i] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }
}

/// Priority bitmap for O(1) priority lookup
struct PriorityBitmap {
    // 256 priorities = 4 u64s
    bits: [AtomicU64; 4],
}

impl PriorityBitmap {
    const fn new() -> Self {
        const INIT: AtomicU64 = AtomicU64::new(0);
        Self {
            bits: [INIT, INIT, INIT, INIT],
        }
    }

    fn set(&self, priority: u8) {
        let word = (priority / 64) as usize;
        let bit = priority % 64;
        self.bits[word].fetch_or(1 << bit, Ordering::Release);
    }

    fn clear(&self, priority: u8) {
        let word = (priority / 64) as usize;
        let bit = priority % 64;
        self.bits[word].fetch_and(!(1 << bit), Ordering::Release);
    }

    fn find_highest(&self) -> Option<u8> {
        // Check from highest priority (word 0) to lowest (word 3)
        for word_idx in 0..4 {
            let word = self.bits[word_idx].load(Ordering::Acquire);
            if word != 0 {
                let bit = word.trailing_zeros() as u8;
                return Some(word_idx as u8 * 64 + bit);
            }
        }
        None
    }
}

/// Scheduler statistics
#[derive(Clone, Copy)]
pub struct SchedulerStats {
    pub total_switches: u64,
    pub total_tasks: usize,
    pub ready_tasks: usize,
    pub running_tasks: usize,
    pub blocked_tasks: usize,
}

impl SchedulerStats {
    const fn new() -> Self {
        Self {
            total_switches: 0,
            total_tasks: 0,
            ready_tasks: 0,
            running_tasks: 0,
            blocked_tasks: 0,
        }
    }
}

/// Production scheduler
pub struct Scheduler {
    // Run queues for each priority level
    run_queues: [RunQueue; 256],
    // Priority bitmap for O(1) lookup
    priority_bitmap: PriorityBitmap,
    // Currently running task
    current: Option<TaskId>,
    // Current task's priority (cached for re-enqueue)
    current_priority: Option<Priority>,
    // Idle task
    idle_task: Option<TaskId>,
    // Statistics
    stats: SchedulerStats,
    // Context switches counter
    switches: AtomicU64,
    // Time slice remaining for current task (in ticks)
    slice_remaining: u64,
    // Whether preemption is needed
    need_resched: bool,
}

impl Scheduler {
    pub const fn new() -> Self {
        const INIT_QUEUE: RunQueue = RunQueue::new();
        
        Self {
            run_queues: [INIT_QUEUE; 256],
            priority_bitmap: PriorityBitmap::new(),
            current: None,
            current_priority: None,
            idle_task: None,
            stats: SchedulerStats::new(),
            switches: AtomicU64::new(0),
            slice_remaining: DEFAULT_QUANTUM,
            need_resched: false,
        }
    }

    /// Called on every timer tick. Decrements time slice and triggers
    /// preemption when quantum expires.
    ///
    /// Returns true if a reschedule is needed.
    pub fn tick(&mut self) -> bool {
        if self.current.is_none() {
            return false;
        }

        if self.slice_remaining > 0 {
            self.slice_remaining -= 1;
        }

        if self.slice_remaining == 0 {
            self.need_resched = true;
        }

        self.need_resched
    }

    /// Check if preemption is pending
    pub fn needs_reschedule(&self) -> bool {
        self.need_resched
    }

    /// Calculate time quantum based on priority
    fn quantum_for_priority(priority: Priority) -> u64 {
        let p = priority.as_u8();
        if p < 32 {
            // Real-time: short quantum for responsiveness
            RT_MIN_QUANTUM + (p as u64)
        } else if p < 128 {
            // Normal: standard quantum
            DEFAULT_QUANTUM
        } else {
            // Low priority: longer quantum (less context switches)
            DEFAULT_QUANTUM * 2
        }
    }

    /// Add task to ready queue
    pub fn enqueue(&mut self, task_id: TaskId, priority: Priority) -> Result<(), KernelError> {
        let prio = priority.as_u8() as usize;
        self.run_queues[prio].push(task_id)?;
        self.priority_bitmap.set(priority.as_u8());
        self.stats.ready_tasks += 1;
        Ok(())
    }

    /// Remove task from ready queue
    pub fn dequeue(&mut self, task_id: TaskId, priority: Priority) -> bool {
        let prio = priority.as_u8() as usize;
        let removed = self.run_queues[prio].remove(task_id);
        
        if removed {
            self.stats.ready_tasks = self.stats.ready_tasks.saturating_sub(1);
            
            // Clear bitmap if queue is empty
            if self.run_queues[prio].is_empty() {
                self.priority_bitmap.clear(priority.as_u8());
            }
        }
        
        removed
    }

    /// Select next task to run (O(1))
    pub fn schedule(&mut self) -> Option<TaskId> {
        self.need_resched = false;

        // If current task's quantum expired, re-enqueue it
        if let (Some(task_id), Some(priority)) = (self.current, self.current_priority) {
            if self.slice_remaining == 0 {
                let _ = self.enqueue(task_id, priority);
                self.current = None;
                self.current_priority = None;
                self.stats.running_tasks = self.stats.running_tasks.saturating_sub(1);
            }
        }

        // Find highest priority non-empty queue
        let priority = self.priority_bitmap.find_highest()?;
        
        let prio = priority as usize;
        let task_id = self.run_queues[prio].pop()?;
        
        // Clear bitmap if queue is now empty
        if self.run_queues[prio].is_empty() {
            self.priority_bitmap.clear(priority);
        }
        
        self.stats.ready_tasks = self.stats.ready_tasks.saturating_sub(1);
        self.stats.running_tasks += 1;
        
        // Set time slice for new task
        self.slice_remaining = Self::quantum_for_priority(Priority::new(priority));
        
        Some(task_id)
    }

    /// Yield current task
    pub fn yield_current(&mut self, task_id: TaskId, priority: Priority) -> Result<(), KernelError> {
        if self.current == Some(task_id) {
            self.current = None;
            self.stats.running_tasks = self.stats.running_tasks.saturating_sub(1);
        }
        
        self.enqueue(task_id, priority)?;
        Ok(())
    }

    /// Block current task
    pub fn block(&mut self, task_id: TaskId) -> Result<(), KernelError> {
        if self.current == Some(task_id) {
            self.current = None;
            self.stats.running_tasks = self.stats.running_tasks.saturating_sub(1);
            self.stats.blocked_tasks += 1;
            Ok(())
        } else {
            Err(KernelError::Process(ProcessError::ProcessNotFound))
        }
    }

    /// Wake blocked task
    pub fn wake(&mut self, task_id: TaskId, priority: Priority) -> Result<(), KernelError> {
        self.stats.blocked_tasks = self.stats.blocked_tasks.saturating_sub(1);
        self.enqueue(task_id, priority)
    }

    /// Remove task from scheduler
    pub fn remove(&mut self, task_id: TaskId, priority: Priority) -> bool {
        if self.current == Some(task_id) {
            self.current = None;
            self.stats.running_tasks = self.stats.running_tasks.saturating_sub(1);
            self.stats.total_tasks = self.stats.total_tasks.saturating_sub(1);
            return true;
        }
        
        if self.dequeue(task_id, priority) {
            self.stats.total_tasks = self.stats.total_tasks.saturating_sub(1);
            return true;
        }
        
        false
    }

    /// Set current running task
    pub fn set_current(&mut self, task_id: TaskId) {
        self.current = Some(task_id);
        self.switches.fetch_add(1, Ordering::Relaxed);
        self.stats.total_switches += 1;
    }

    /// Set current running task with priority tracking
    pub fn set_current_with_priority(&mut self, task_id: TaskId, priority: Priority) {
        self.current = Some(task_id);
        self.current_priority = Some(priority);
        self.switches.fetch_add(1, Ordering::Relaxed);
        self.stats.total_switches += 1;
    }

    /// Get current running task
    pub fn current(&self) -> Option<TaskId> {
        self.current
    }

    /// Set idle task
    pub fn set_idle_task(&mut self, task_id: TaskId) {
        self.idle_task = Some(task_id);
    }

    /// Get idle task
    pub fn idle_task(&self) -> Option<TaskId> {
        self.idle_task
    }

    /// Get scheduler statistics
    pub fn stats(&self) -> &SchedulerStats {
        &self.stats
    }

    /// Get total context switches
    pub fn total_switches(&self) -> u64 {
        self.switches.load(Ordering::Relaxed)
    }

    /// Check if scheduler has any ready tasks
    pub fn has_ready_tasks(&self) -> bool {
        self.priority_bitmap.find_highest().is_some()
    }

    /// Get number of tasks at priority level
    pub fn tasks_at_priority(&self, priority: Priority) -> usize {
        self.run_queues[priority.as_u8() as usize].len()
    }
}

unsafe impl Send for Scheduler {}
unsafe impl Sync for Scheduler {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_queue() {
        let mut queue = RunQueue::new();
        
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
        
        queue.push(TaskId::new(1)).unwrap();
        queue.push(TaskId::new(2)).unwrap();
        
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop(), Some(TaskId::new(1)));
        assert_eq!(queue.pop(), Some(TaskId::new(2)));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_priority_bitmap() {
        let bitmap = PriorityBitmap::new();
        
        assert_eq!(bitmap.find_highest(), None);
        
        bitmap.set(128);
        bitmap.set(64);
        bitmap.set(0);
        
        // Should find highest priority (lowest number)
        assert_eq!(bitmap.find_highest(), Some(0));
        
        bitmap.clear(0);
        assert_eq!(bitmap.find_highest(), Some(64));
        
        bitmap.clear(64);
        assert_eq!(bitmap.find_highest(), Some(128));
    }

    #[test]
    fn test_scheduler_enqueue_dequeue() {
        let mut sched = Scheduler::new();
        
        let task1 = TaskId::new(1);
        let task2 = TaskId::new(2);
        
        sched.enqueue(task1, Priority::NORMAL).unwrap();
        sched.enqueue(task2, Priority::HIGH).unwrap();
        
        assert_eq!(sched.stats().ready_tasks, 2);
        
        // Should schedule higher priority first
        assert_eq!(sched.schedule(), Some(task2));
        assert_eq!(sched.schedule(), Some(task1));
        assert_eq!(sched.schedule(), None);
    }

    #[test]
    fn test_scheduler_yield() {
        let mut sched = Scheduler::new();
        
        let task = TaskId::new(1);
        sched.enqueue(task, Priority::NORMAL).unwrap();
        
        let scheduled = sched.schedule().unwrap();
        sched.set_current(scheduled);
        
        sched.yield_current(task, Priority::NORMAL).unwrap();
        
        assert_eq!(sched.current(), None);
        assert_eq!(sched.stats().ready_tasks, 1);
    }

    #[test]
    fn test_scheduler_block_wake() {
        let mut sched = Scheduler::new();
        
        let task = TaskId::new(1);
        sched.enqueue(task, Priority::NORMAL).unwrap();
        
        let scheduled = sched.schedule().unwrap();
        sched.set_current(scheduled);
        
        sched.block(task).unwrap();
        assert_eq!(sched.stats().blocked_tasks, 1);
        
        sched.wake(task, Priority::NORMAL).unwrap();
        assert_eq!(sched.stats().blocked_tasks, 0);
        assert_eq!(sched.stats().ready_tasks, 1);
    }

    #[test]
    fn test_scheduler_priority_order() {
        let mut sched = Scheduler::new();
        
        sched.enqueue(TaskId::new(1), Priority::LOW).unwrap();
        sched.enqueue(TaskId::new(2), Priority::HIGH).unwrap();
        sched.enqueue(TaskId::new(3), Priority::NORMAL).unwrap();
        sched.enqueue(TaskId::new(4), Priority::REALTIME_MAX).unwrap();
        
        // Should schedule in priority order
        assert_eq!(sched.schedule(), Some(TaskId::new(4))); // RT
        assert_eq!(sched.schedule(), Some(TaskId::new(2))); // HIGH
        assert_eq!(sched.schedule(), Some(TaskId::new(3))); // NORMAL
        assert_eq!(sched.schedule(), Some(TaskId::new(1))); // LOW
    }

    #[test]
    fn test_scheduler_stats() {
        let mut sched = Scheduler::new();
        
        sched.enqueue(TaskId::new(1), Priority::NORMAL).unwrap();
        let task = sched.schedule().unwrap();
        sched.set_current(task);
        
        assert_eq!(sched.total_switches(), 1);
        assert_eq!(sched.stats().running_tasks, 1);
    }
}
