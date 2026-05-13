//! Timer-based sleep queue for the STAR OS kernel.
//!
//! Maintains a sorted array of sleeping tasks, ordered by wakeup tick.
//! Max 128 concurrent sleeping tasks. No heap allocation required.

use crate::prelude::*;

const MAX_SLEEPING: usize = 128;

#[derive(Clone, Copy)]
struct SleepEntry {
    task_id: u64,
    wakeup_tick: u64,
}

pub struct SleepQueue {
    entries: [Option<SleepEntry>; MAX_SLEEPING],
    count: usize,
}

impl SleepQueue {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_SLEEPING],
            count: 0,
        }
    }

    /// Insert a task into the queue, sorted by wakeup_tick.
    /// Returns false if the queue is full.
    pub fn sleep_until(&mut self, task_id: u64, wakeup_tick: u64) -> bool {
        if self.count >= MAX_SLEEPING {
            return false;
        }

        // Find insertion point (sorted ascending by wakeup_tick)
        let mut pos = self.count;
        for i in 0..self.count {
            if self.entries[i].unwrap().wakeup_tick > wakeup_tick {
                pos = i;
                break;
            }
        }

        // Shift entries right
        for i in (pos..self.count).rev() {
            self.entries[i + 1] = self.entries[i];
        }

        self.entries[pos] = Some(SleepEntry { task_id, wakeup_tick });
        self.count += 1;
        true
    }

    /// Convenience: sleep for a duration relative to current tick.
    pub fn sleep_for(&mut self, task_id: u64, current_tick: u64, duration_ticks: u64) -> bool {
        self.sleep_until(task_id, current_tick + duration_ticks)
    }

    /// Called on each timer tick. Returns task_ids whose wakeup time has arrived.
    pub fn tick(&mut self, current_tick: u64) -> Vec<u64> {
        let mut woken = Vec::new();
        let mut woken_count = 0;

        for i in 0..self.count {
            if self.entries[i].unwrap().wakeup_tick <= current_tick {
                woken.push(self.entries[i].unwrap().task_id);
                woken_count += 1;
            } else {
                break; // sorted, so no more expired entries
            }
        }

        if woken_count > 0 {
            // Shift remaining entries to front
            for i in 0..(self.count - woken_count) {
                self.entries[i] = self.entries[i + woken_count];
            }
            for i in (self.count - woken_count)..self.count {
                self.entries[i] = None;
            }
            self.count -= woken_count;
        }

        woken
    }

    /// Remove a task from the sleep queue (e.g., on signal delivery).
    pub fn cancel_sleep(&mut self, task_id: u64) -> bool {
        for i in 0..self.count {
            if self.entries[i].unwrap().task_id == task_id {
                // Shift left
                for j in i..(self.count - 1) {
                    self.entries[j] = self.entries[j + 1];
                }
                self.entries[self.count - 1] = None;
                self.count -= 1;
                return true;
            }
        }
        false
    }

    /// Number of tasks currently sleeping.
    pub fn sleeping_count(&self) -> usize {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep_and_tick() {
        let mut q = SleepQueue::new();
        assert!(q.sleep_until(1, 100));
        assert!(q.sleep_until(2, 50));
        assert!(q.sleep_for(3, 40, 30)); // wakes at 70
        assert_eq!(q.sleeping_count(), 3);

        let woken = q.tick(60);
        assert_eq!(woken, vec![2]);
        assert_eq!(q.sleeping_count(), 2);

        let woken = q.tick(100);
        assert_eq!(woken, vec![3, 1]);
        assert_eq!(q.sleeping_count(), 0);
    }

    #[test]
    fn test_cancel() {
        let mut q = SleepQueue::new();
        q.sleep_until(5, 200);
        q.sleep_until(6, 300);
        assert!(q.cancel_sleep(5));
        assert_eq!(q.sleeping_count(), 1);
        assert!(!q.cancel_sleep(5));
    }

    #[test]
    fn test_full_queue() {
        let mut q = SleepQueue::new();
        for i in 0..128 {
            assert!(q.sleep_until(i, i * 10));
        }
        assert!(!q.sleep_until(999, 5000));
    }
}
