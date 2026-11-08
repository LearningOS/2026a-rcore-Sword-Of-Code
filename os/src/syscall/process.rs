//! Process management syscalls
use crate::config::PAGE_SIZE;
use crate::mm::{
    translated_byte_buffer, MapPermission, PTEFlags, PageTable, VirtAddr,
};
use crate::task::{
    change_program_brk, current_mmap, current_munmap, current_user_token,
    exit_current_and_run_next, get_syscall_count, suspend_current_and_run_next,
};
use crate::timer::get_time_us;

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

/// Get time of day, writing the result through a user-space pointer that
/// may straddle a page boundary.
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let src = unsafe {
        core::slice::from_raw_parts(
            &time_val as *const TimeVal as *const u8,
            core::mem::size_of::<TimeVal>(),
        )
    };
    let buffers = translated_byte_buffer(
        current_user_token(),
        ts as *const u8,
        core::mem::size_of::<TimeVal>(),
    );
    let mut offset = 0;
    for buf in buffers {
        let len = buf.len();
        buf.copy_from_slice(&src[offset..offset + len]);
        offset += len;
    }
    0
}

/// trace: read/write a single user byte, or read the current task's
/// invocation count for a given syscall id.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => {
            let va = VirtAddr::from(id);
            let vpn = va.floor();
            let page_table = PageTable::from_token(current_user_token());
            if let Some(pte) = page_table.translate(vpn) {
                if pte.is_valid()
                    && pte.readable()
                    && pte.flags().contains(PTEFlags::U)
                {
                    let byte = pte.ppn().get_bytes_array()[va.page_offset()];
                    return byte as isize;
                }
            }
            -1
        }
        1 => {
            let va = VirtAddr::from(id);
            let vpn = va.floor();
            let page_table = PageTable::from_token(current_user_token());
            if let Some(pte) = page_table.translate(vpn) {
                if pte.is_valid()
                    && pte.writable()
                    && pte.flags().contains(PTEFlags::U)
                {
                    pte.ppn().get_bytes_array()[va.page_offset()] = data as u8;
                    return 0;
                }
            }
            -1
        }
        2 => get_syscall_count(id) as isize,
        _ => -1,
    }
}

/// Anonymously map `[start, start + len)` into the current task's address
/// space with the given `prot` (bit0=R, bit1=W, bit2=X).
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if prot & !0x7 != 0 {
        return -1;
    }
    if prot & 0x7 == 0 {
        return -1;
    }
    let mut permission = MapPermission::U;
    if prot & 0x1 != 0 {
        permission |= MapPermission::R;
    }
    if prot & 0x2 != 0 {
        permission |= MapPermission::W;
    }
    if prot & 0x4 != 0 {
        permission |= MapPermission::X;
    }
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    current_mmap(start_va, end_va, permission)
}

/// Unmap `[start, start + len)` from the current task's address space.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    let start_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    current_munmap(start_va, end_va)
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
