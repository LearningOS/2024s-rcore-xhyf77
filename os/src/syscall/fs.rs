//! File and filesystem-related syscalls

use crate::fs::ROOT_INODE;
use crate::fs::{open_file, OpenFlags, Stat};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

use crate::fs::StatMode;
use crate::mm::VirtAddr;
use crate::mm::PageTable;
use crate::mm::PhysAddr;
use alloc::vec;
/// YOUR JOB: Implement fstat.
pub fn sys_fstat( fd: usize, _st: *mut Stat) -> isize {
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    let file = inner.fd_table[fd].as_ref().unwrap().clone();
    drop(inner);
    let osinode = file.get_osinode().unwrap();
    let inode = osinode.inner.exclusive_access().inode.clone();
    let _id = inode.get_inode_id();
    let _link_time = osinode.inner.exclusive_access().inode.read_disk_inode(|diskinode| diskinode.nlink);
    
    let mut stmd = StatMode::NULL;
    if _id.1 == 0o040000 {
        stmd = StatMode::DIR;
    }
    else if _id.1 == 0o100000 {
        stmd = StatMode::FILE;
    }

    let stat_kernel = Stat {
        dev: 0 ,
        ino : _id.0 ,
        mode : stmd ,
        nlink : _link_time as u32 ,
        pad : [0 as u64 ; 7] ,
    };

    let st_r = unsafe { _st.as_mut().unwrap() }; 
    let ptr1:VirtAddr = (&st_r.dev as *const _ as usize).into();
    let ptr2:VirtAddr = (&st_r.ino as *const _ as usize).into();
    let ptr3:VirtAddr = (&st_r.mode as *const _ as usize).into();
    let ptr4:VirtAddr = (&st_r.nlink as *const _ as usize).into();

    let pt = current_user_token();
    let page_table = PageTable {
        root_ppn: pt.into(),
        frames:vec![]
    };

    let pgaddr1: PhysAddr = page_table.find_pte( ptr1.floor()).unwrap().ppn().into();
    let pgaddr2: PhysAddr = page_table.find_pte( ptr2.floor()).unwrap().ppn().into();
    let pgaddr3: PhysAddr = page_table.find_pte( ptr3.floor()).unwrap().ppn().into();
    let pgaddr4: PhysAddr = page_table.find_pte( ptr4.floor()).unwrap().ppn().into();

    let ptr11 = ( pgaddr1.0 + ( (&st_r.dev as *const u64 as usize) & 0xfff ) ) as *mut u64;
    let ptr22 = ( pgaddr2.0 + ( (&st_r.ino as *const _ as usize) & 0xfff ) ) as *mut u64;
    let ptr33 = ( pgaddr3.0 + ( (&st_r.mode as *const _ as usize) & 0xfff ) ) as *mut StatMode;
    let ptr44 = ( pgaddr4.0 + ( (&st_r.nlink as *const _ as usize) & 0xfff ) ) as *mut u32;

    unsafe{
        *ptr11 = stat_kernel.dev;
        *ptr22 = stat_kernel.ino;
        *ptr33 = stat_kernel.mode;
        *ptr44 = stat_kernel.nlink;
    }
    0
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    let token = current_user_token();
    let rt = ROOT_INODE.clone();
    let oldpath = translated_str(token, _old_name);
    let newpath = translated_str(token, _new_name);
    let x = rt.copy(oldpath.as_str(), newpath.as_str()) as isize;
    x
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(_name: *const u8) -> isize {
    let token = current_user_token();
    let rt = ROOT_INODE.clone();
    let path = translated_str(token, _name);
    rt.unlink(path.as_str()) as isize
}
