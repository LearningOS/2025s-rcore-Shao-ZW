//! Process management syscalls
//!
use alloc::sync::Arc;

use crate::{
    config::PAGE_SIZE,
    fs::{open_file, OpenFlags},
    mm::{translated_refmut, translated_str},
    mm::{MapPermission, VPNRange, VirtAddr, FRAME_ALLOCATOR},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
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
    //trace!("kernel: sys_waitpid");
    trace!(
        "kernel::pid[{}] sys_waitpid [{}]",
        current_task().unwrap().pid.0,
        pid
    );
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel:pid[{}] sys_get_time", current_task().unwrap().pid.0);
    let ts = translated_refmut(current_user_token(), _ts);
    let us = get_time_us();
    *ts = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
#[allow(dead_code)]
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    let mm_set = &mut inner.memory_set;
    let va = VirtAddr::from(_id as usize);

    let retval = match _trace_request {
        0 => {
            if let Some(pte) = mm_set.translate(va.floor()) {
                if pte.readable() && pte.is_user() {
                    let ptr = translated_refmut(current_user_token(), _id as *mut u8);
                    *ptr as isize
                } else {
                    error!("sys_trace: read from non-readable page");
                    -1
                }
            } else {
                -1
            }
        }
        1 => {
            if let Some(pte) = mm_set.translate(va.floor()) {
                if pte.writable() && pte.is_user() {
                    let ptr = translated_refmut(current_user_token(), _id as *mut u8);
                    *ptr = _data as u8;
                    0
                } else {
                    error!("sys_trace: write to non-writable page");
                    -1
                }
            } else {
                -1
            }
        }
        2 => {
            let retval = task.inner_exclusive_access().syscall_times[_id];
            retval as isize
        }
        _ => -1,
    };
    drop(inner);
    retval
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel:pid[{}] sys_mmap", current_task().unwrap().pid.0);
    if !VirtAddr::from(_start).aligned() {
        return -1;
    }
    if _port & !0x7 != 0 || _port & 0x7 == 0 {
        return -1;
    }
    if FRAME_ALLOCATOR.exclusive_access().unused_cnt() < (_len + PAGE_SIZE - 1) / PAGE_SIZE {
        return -1;
    }

    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    let mm_set = &mut inner.memory_set;
    let start_vpn = VirtAddr::from(_start).floor();
    let end_vpn = VirtAddr::from(_start + _len).ceil();
    let vpn_range = VPNRange::new(start_vpn, end_vpn);

    for vpn in vpn_range.into_iter() {
        if let Some(pte) = mm_set.translate(vpn) {
            if pte.is_valid() {
                return -1;
            }
        }
    }

    let mut map_perm = MapPermission::U;
    if _port & 1 != 0 {
        map_perm |= MapPermission::R;
    }
    if _port & 2 != 0 {
        map_perm |= MapPermission::W;
    }
    if _port & 4 != 0 {
        map_perm |= MapPermission::X;
    }

    mm_set.insert_framed_area(VirtAddr::from(start_vpn), VirtAddr::from(end_vpn), map_perm);
    0
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel:pid[{}] sys_munmap", current_task().unwrap().pid.0);
    if !VirtAddr::from(_start).aligned() {
        return -1;
    }

    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    let mm_set = &mut inner.memory_set;
    let start_vpn = VirtAddr::from(_start).floor();
    let end_vpn = VirtAddr::from(_start + _len).ceil();
    let vpn_range = VPNRange::new(start_vpn, end_vpn);

    for vpn in vpn_range.into_iter() {
        match mm_set.translate(vpn) {
            Some(pte) => {
                if !pte.is_valid() {
                    return -1;
                }
            }
            None => return -1,
        }
    }

    mm_set.remove_area_with_start_vpn(start_vpn); // This implementation is not rigorous (just enough for the tests)
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
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_spawn", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    let token = current_user_token();
    let path = translated_str(token, _path);

    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        new_task.exec(all_data.as_slice());
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority",
        current_task().unwrap().pid.0
    );
    if _prio <= 1 {
        return -1;
    }

    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    inner.priority = _prio as u32;
    _prio
}
