//! Process management syscalls
use alloc::vec;

use crate::{
    config::{MAX_SYSCALL_NUM, PAGE_SIZE_BITS}, mm::{PageTable, PhysAddr, VirtAddr}, task::{
        change_program_brk, current_user_token, exit_current_and_run_next, suspend_current_and_run_next, TaskStatus, TASK_MANAGER
    }, timer::{get_time, get_time_ms, get_time_us}
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

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
   // println!("---{:x}" , _ts as usize );
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
   // println!("a--{:x},{:x}" , ptr11 as usize , ptr22 as usize );
   // println!("b--{:x}",( _ts as usize & 0x111 ));
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    let t = get_time_ms();
    let ti_r = unsafe { _ti.as_mut().unwrap() }; 
    let time = t - TASK_MANAGER.get_first_runtime();
    let syscall_times = TASK_MANAGER.get_syscall_times();
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

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    //println!("---------------------------------------------------" );
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

    TASK_MANAGER.create_alloc(_start, _len, _port);
    let y : VirtAddr = _start.into();
    let x = page_table.find_pte( y.floor()).unwrap().ppn();
    //println!("{:x}",y.floor().0);
    //println!("{:x}", x.0 );
    0
}

// YOUR JOB: Implement munmap.
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

    TASK_MANAGER.unmapp(svirtaddr, evirtaddr );
    0
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
