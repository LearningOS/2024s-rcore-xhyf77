use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task, TaskControlBlock};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use super::sys_gettid;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
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
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}

fn set_need_resource(flag: bool, id: usize) {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    let _ = inner.need.replace((flag, id)).is_none();
}

fn mark_resource_released(flag: bool, id: usize) {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if let Some(pos) = inner
        .allocation
        .iter()
        .position(|res| res.0 == flag && res.1 == id)
    {
        inner.allocation.swap_remove(pos);
    }
}

/// deadlock detectiong algorithm
fn detect_deadlock(
    tasks: &Vec<Option<Arc<TaskControlBlock>>>,
    mutexes: &Vec<Option<Arc<dyn Mutex>>>,
    semaphores: &Vec<Option<Arc<Semaphore>>>,
    use_mutex: bool,
    index: usize,
) -> bool {
    let num_resources = mutexes.len() + semaphores.len();

    let mut allocation_matrix = vec![vec![0; num_resources]; tasks.len()];
    let mut need_matrix = vec![vec![0; num_resources]; tasks.len()];
    let mut work_vector = vec![0; num_resources];

    let fetch_resource_id = |use_mutex, index| -> usize {
        if use_mutex {
            index + mutexes.len()
        } else {
            index
        }
    };

    for (task_id, task) in tasks.iter().enumerate() {
        if let Some(task) = task {
            let inner = task.inner_exclusive_access();
            for allocation_info in &inner.allocation {
                let resource_id = fetch_resource_id(allocation_info.0, allocation_info.1);
                allocation_matrix[task_id][resource_id] += 1;
            }
            if let Some(need_info) = &inner.need {
                let resource_id = fetch_resource_id(need_info.0, need_info.1);
                need_matrix[task_id][resource_id] += 1;
            }
        }
    }
    let current_task_id = sys_gettid() as usize;
    let resource_id = fetch_resource_id(use_mutex, index);
    need_matrix[current_task_id][resource_id] += 1;

    for (mutex_id, mutex_option) in mutexes.iter().enumerate() {
        if let Some(mutex) = mutex_option {
            if !mutex.is_locked() {
                work_vector[mutex_id] += 1;
            }
        }
    }
    for (semaphore_id, semaphore_option) in semaphores.iter().enumerate() {
        if let Some(semaphore) = semaphore_option {
            let count = semaphore.inner.exclusive_access().count;
            if count > 0 {
                let resource_id = semaphore_id + mutexes.len();
                work_vector[resource_id] += count as u32;
            }
        }
    }

    let mut finished_tasks = vec![false; tasks.len()];
    loop {
        let unfinished_task = finished_tasks.iter().enumerate().find(|(task_id, finished)| {
            if **finished {
                false
            } else {
                for resource_id in 0..num_resources {
                    if need_matrix[*task_id][resource_id] > work_vector[resource_id] {
                        return false;
                    }
                }
                true
            }
        });
        if let Some((task_id, _)) = unfinished_task {
            finished_tasks[task_id] = true;
            for resource_id in 0..num_resources {
                work_vector[resource_id] += allocation_matrix[task_id][resource_id];
            }
        } else {
            break;
        }
    }
    finished_tasks.contains(&false)
}

/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    if process_inner.deaddetection && detect_deadlock(
            &process_inner.tasks,
            &process_inner.mutex_list,
            &process_inner.semaphore_list,
            false,
            mutex_id,
        )
    {
        return -0xDEAD;
    }
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    set_need_resource(false, mutex_id);
    mutex.lock();
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mark_resource_released(false, mutex_id);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
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
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    mark_resource_released(true, sem_id);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    if process_inner.deaddetection
        && detect_deadlock(
            &process_inner.tasks,
            &process_inner.mutex_list,
            &process_inner.semaphore_list,
            true,
            sem_id,
        )
    {
        return -0xDEAD;
    }
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    set_need_resource(true, sem_id);
    sem.down();
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
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
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
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
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect( enabled: usize) -> isize {
    let proc = current_process();
    let mut inner = proc.inner_exclusive_access();
    if enabled == 0 {
        inner.deaddetection = false;
    }
    else{
        inner.deaddetection = true;
    }
    0
}
