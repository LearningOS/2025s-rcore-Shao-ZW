//! Process management syscalls
use core::panic;

use crate::{
    config::PAGE_SIZE,
     mm::{MapPermission, PhysAddr, VPNRange, VirtAddr, FRAME_ALLOCATOR},
     task::{
         change_program_brk, exit_current_and_run_next, suspend_current_and_run_next ,TASK_MANAGER,
     },
     timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
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
    trace!("kernel: sys_get_time");
    let inner = TASK_MANAGER.inner.exclusive_access();
    let current = inner.current_task;
    let mm_set = &inner.tasks[current].memory_set;
    let va = VirtAddr::from(_ts as usize);
    
    if let Some(pte) = mm_set.translate(va.floor()) {
        let pa = va.form_pa(pte);
        let ts_pa = usize::from(pa) as *mut TimeVal;
        let us = get_time_us();
        unsafe {
            *ts_pa = TimeVal {
                sec: us / 1_000_000,
                usec: us % 1_000_000,
            };
        }
        drop(inner);
        return 0;
    }
    -1
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    let inner = TASK_MANAGER.inner.exclusive_access();
    let task = &inner.tasks[inner.current_task];
    let mm_set = &task.memory_set;
    let va = VirtAddr::from(_id as usize);
    
    let retval = match _trace_request {
        0 => {
            if let Some(pte) = mm_set.translate(va.floor()) {
                if pte.readable() && pte.is_user() {
                    let pa: PhysAddr = va.form_pa(pte);
                    let ptr = usize::from(pa) as *const u8;
                    unsafe { *ptr as isize }
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
                    let pa: PhysAddr = va.form_pa(pte);
                    let ptr = usize::from(pa) as *mut u8;
                    unsafe { *ptr = _data as u8 }
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
            let current = inner.current_task;
            let retval = inner.tasks[current].syscall_times[_id];
            retval as isize
        }
        _=> {
            -1
        }
    };
    drop(inner);
    retval
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap");
 
    if !VirtAddr::from(_start).aligned() {
        return -1;
    }
    if _port & !0x7 != 0 || _port & 0x7 == 0 {
        return -1;
    }
    if FRAME_ALLOCATOR.exclusive_access().unused_cnt() < (_len + PAGE_SIZE - 1) / PAGE_SIZE {
        return -1;
    }

    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let current = inner.current_task;
    let mm_set = &mut inner.tasks[current].memory_set;
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
    drop(inner);
    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap");
 
    if !VirtAddr::from(_start).aligned() {
        return -1;
    }

    let mut inner = TASK_MANAGER.inner.exclusive_access();
    let current = inner.current_task;
    let mm_set = &mut inner.tasks[current].memory_set;
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

    mm_set.delete_area(VirtAddr::from(start_vpn), VirtAddr::from(end_vpn));
    drop(inner);
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
