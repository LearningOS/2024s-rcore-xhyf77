//! Process management syscalls
//!
use alloc::sync::Arc;
use crate::task::TaskControlBlock;
use crate::{
    config::MAX_SYSCALL_NUM,
    fs::{open_file, OpenFlags},
    mm::{translated_refmut, translated_str},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, TaskStatus,
    },
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task: Arc<TaskControlBlock> = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        let exit_code = child.inner_exclusive_access().exit_code;
        println!("pid:{},texi_code:{}" , pid , exit_code);
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }

}
use crate::timer::get_time_us;
/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let ptr1:VirtAddr = (_ts as usize).into();
    let ptr2:VirtAddr = (_ts as usize + 8).into();
    let ux = get_time_us();
    let sx = ux / 1000000;
    let pt = current_user_token();
    let page_table = PageTable {
        root_ppn: pt.into(),
        frames:vec![]
    };
    let pgaddr1: PhysAddr = page_table.find_pte( ptr1.floor()).unwrap().ppn().into();
    let pgaddr2: PhysAddr = page_table.find_pte( ptr2.floor()).unwrap().ppn().into();
    let ptr11 = ( pgaddr1.0 + ( _ts as usize & 0xfff ) ) as *mut usize;
    let ptr22 = ( pgaddr2.0 + ( (_ts as usize + 8 ) & 0xfff ) ) as *mut usize;
    unsafe{
        *ptr11 = sx;
        *ptr22 = ux;
    }
    0
}
use crate::timer::get_time_ms;
use crate::task::PROCESSOR;
use crate::mm::VirtAddr;
use crate::mm::PhysAddr;
use crate::mm::PageTable;
use alloc::vec;
/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    let process = PROCESSOR.exclusive_access();
    let t = get_time_ms();
    let ti_r = unsafe { _ti.as_mut().unwrap() }; 
    let time = t - process.get_first_runtime();
    let syscall_times = process.get_syscall_times();

    drop(process);

    let status = TaskStatus::Running;

    let ptr1:VirtAddr = (&ti_r.time as *const usize as usize).into();
    let ptr2:VirtAddr = (&ti_r.syscall_times as *const _ as usize).into();
    let ptr3:VirtAddr = (&ti_r.status as *const _ as usize).into();

    let pt = current_user_token();
    let page_table = PageTable {
        root_ppn: pt.into(),
        frames:vec![]
    };

    let pgaddr1: PhysAddr = page_table.find_pte( ptr1.floor()).unwrap().ppn().into();
    let pgaddr2: PhysAddr = page_table.find_pte( ptr2.floor()).unwrap().ppn().into();
    let pgaddr3: PhysAddr = page_table.find_pte( ptr3.floor()).unwrap().ppn().into();

    let ptr11 = ( pgaddr1.0 + ( (&ti_r.time as *const usize as usize) & 0xfff ) ) as *mut usize;
    let ptr22 = ( pgaddr2.0 + ( (&ti_r.syscall_times as *const _ as usize) & 0xfff ) ) as *mut u32 as *mut [u32;500];
    let ptr33 = ( pgaddr3.0 + ( (&ti_r.status as *const _ as usize) & 0xfff ) ) as *mut TaskStatus;
    unsafe{
        *ptr11 = time;
        *ptr22 = syscall_times;
        *ptr33 = status;
    }
    0
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    let svirtaddr : VirtAddr = _start.into();
    if svirtaddr.page_offset() != 0 || _port & !0x7 !=0 || _port & 0x7 == 0 {
        return -1;
    }

    let evirtaddr : VirtAddr = (_start + _len).into();
    let pt = current_user_token();

    let page_table = PageTable {
        root_ppn: pt.into(),
        frames:vec![]
    };

    let ( _all_mapp , all_unmapp) = page_table.if_has_mapped( svirtaddr , evirtaddr );
    if all_unmapp == -1 {
        return -1;
    }
    let processor = PROCESSOR.exclusive_access();
    processor.create_alloc(_start, _len, _port);

    drop(processor);
    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    let svirtaddr : VirtAddr = _start.into();
    if svirtaddr.page_offset() != 0 {
        return -1;
    }

    let evirtaddr : VirtAddr = (_start + _len).into();
    let pt = current_user_token();
    let page_table = PageTable {
        root_ppn: pt.into(),
        frames:vec![]
    };

    let ( all_mapp , _all_unmapp) = page_table.if_has_mapped( svirtaddr , evirtaddr );
    if all_mapp == -1 {
        return -1;
    }
    let processor = PROCESSOR.exclusive_access();
    processor.unmapp(svirtaddr, evirtaddr );

    drop(processor);
    0
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn( path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    let fd = open_file(path.as_str(),OpenFlags::RDONLY );
    if fd.is_none() {
        println!("here");
        return -1;
    }
    let data = &fd.unwrap().read_all()[..];
    
    let childtask = Arc::new(TaskControlBlock::new(data));
    let parenttask = current_task().unwrap();
    let mut parent_inner = parenttask.inner_exclusive_access();
    let mut child_inner = childtask.inner_exclusive_access();
    child_inner.parent = Some(Arc::downgrade(&parenttask));
    parent_inner.children.push(childtask.clone());
    add_task(childtask.clone());
    return  childtask.getpid() as isize;
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority( prio: isize) -> isize {
    if prio < 2 {
        return -1;
    }
    let processor = PROCESSOR.exclusive_access();
    let x = processor.current().unwrap();
    x.inner_exclusive_access().priority = prio;
    prio
}
