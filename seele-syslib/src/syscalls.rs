use core::ffi::c_char;

use crate::{syscall, utils::SyscallResult};

pub mod filesystem;
pub mod futex;
pub mod object;
pub mod polling;

// Wait for a process to exit
pub fn wait_for_process_exit(process: i32, exit_code_ptr: *mut i32) -> SyscallResult {
    syscall!(WaitForProcessExit, process as u64, exit_code_ptr as u64)
}

pub fn exit(code: u64) -> SyscallResult {
    syscall!(Exit, code)
}

pub fn fork() -> SyscallResult {
    syscall!(Fork)
}

pub fn set_fs(addr: u64) -> SyscallResult {
    syscall!(SetFs, addr)
}

pub fn get_fs() -> SyscallResult {
    syscall!(GetFs)
}

pub fn set_gs(addr: u64) -> SyscallResult {
    syscall!(SetGs, addr)
}

fn allocate_mem_pages(pages: u64, flags: u64) -> SyscallResult {
    syscall!(AllocateMem, pages, flags)
}

pub fn allocate_mem(len: u64, flags: u64) -> SyscallResult {
    allocate_mem_pages((len + 4095) / 4096, flags)
}

pub fn get_process_id() -> SyscallResult {
    syscall!(GetProcessID)
}

pub fn get_process_parent_id() -> SyscallResult {
    syscall!(GetProcessParentID)
}

pub fn get_thread_id() -> SyscallResult {
    // TODO not yet implemented
    get_process_id()
}

pub fn execve(path: &str, args: *const *mut c_char, env: *const *mut c_char) -> SyscallResult {
    syscall!(
        Execve,
        path.as_bytes().as_ptr() as u64,
        args as u64,
        env as u64
    )
}
