//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;

/// A fixed constant used to derive each process's per-run stride step:
/// `pass = BIG_STRIDE / priority`. A larger BIG_STRIDE reduces truncation
/// error in that division at the cost of faster stride growth. Picked small
/// enough that u64 stride accumulation does not overflow in any reasonable
/// test run.
pub const BIG_STRIDE: u64 = 0x10_0000;

///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// Stride scheduler: picks the Ready task with the smallest stride, then
/// advances that task's stride by `BIG_STRIDE / priority`.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take the min-stride process out of the ready queue and advance its
    /// stride by its pass.
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if self.ready_queue.is_empty() {
            return None;
        }
        let mut idx = 0usize;
        let mut min_stride = u64::MAX;
        for (i, task) in self.ready_queue.iter().enumerate() {
            let s = task.inner_exclusive_access().stride;
            if s < min_stride {
                min_stride = s;
                idx = i;
            }
        }
        let task = self.ready_queue.remove(idx).unwrap();
        let mut inner = task.inner_exclusive_access();
        let pass = BIG_STRIDE / inner.priority as u64;
        inner.stride = inner.stride.wrapping_add(pass);
        drop(inner);
        Some(task)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
