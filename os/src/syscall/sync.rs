use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;

/// Error code returned when deadlock detection refuses a request.
const EDEADLK: isize = -0xDEAD;

fn current_tid() -> usize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid
}

/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() - 1
    };
    for row in process_inner.mutex_alloc.iter_mut() {
        while row.len() <= id {
            row.push(0);
        }
    }
    id as isize
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let tid = current_tid();
    let process = current_process();
    let mutex = {
        let mut process_inner = process.inner_exclusive_access();
        process_inner.ensure_mutex_entry(tid);
        if process_inner.deadlock_detect_enabled
            && !process_inner.is_mutex_safe(tid, mutex_id)
        {
            return EDEADLK;
        }
        Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap())
    };
    mutex.lock();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.ensure_mutex_entry(tid);
    process_inner.mutex_alloc[tid][mutex_id] += 1;
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let tid = current_tid();
    let process = current_process();
    let mutex = {
        let mut process_inner = process.inner_exclusive_access();
        process_inner.ensure_mutex_entry(tid);
        if let Some(cell) = process_inner
            .mutex_alloc
            .get_mut(tid)
            .and_then(|row| row.get_mut(mutex_id))
        {
            *cell = cell.saturating_sub(1);
        }
        Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap())
    };
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    for row in process_inner.sem_alloc.iter_mut() {
        while row.len() <= id {
            row.push(0);
        }
    }
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let tid = current_tid();
    let process = current_process();
    let sem = {
        let mut process_inner = process.inner_exclusive_access();
        process_inner.ensure_sem_entry(tid);
        if let Some(cell) = process_inner
            .sem_alloc
            .get_mut(tid)
            .and_then(|row| row.get_mut(sem_id))
        {
            *cell = cell.saturating_sub(1);
        }
        Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap())
    };
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let tid = current_tid();
    let process = current_process();
    let sem = {
        let mut process_inner = process.inner_exclusive_access();
        process_inner.ensure_sem_entry(tid);
        if process_inner.deadlock_detect_enabled
            && !process_inner.is_sem_safe(tid, sem_id)
        {
            return EDEADLK;
        }
        Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap())
    };
    sem.down();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.ensure_sem_entry(tid);
    process_inner.sem_alloc[tid][sem_id] += 1;
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid()
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}

/// Enable or disable deadlock detection for the current process.
///
/// `is_enable`: 1 to enable, 0 to disable. Returns 0 on success or -1 on invalid input.
pub fn sys_enable_deadlock_detect(is_enable: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_enable_deadlock_detect({})",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_tid(),
        is_enable
    );
    match is_enable {
        0 | 1 => {
            let process = current_process();
            process.inner_exclusive_access().deadlock_detect_enabled = is_enable == 1;
            0
        }
        _ => -1,
    }
}
