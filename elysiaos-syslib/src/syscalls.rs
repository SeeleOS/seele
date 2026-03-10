use crate::{errors::SyscallError, numbers::SyscallNumber, syscall, utils::SyscallResult, wrap_c};

pub mod filesystem;
pub mod futex;
pub mod object;

pub fn exit(code: u64) -> SyscallResult {
    syscall!(Exit, code)
}

pub fn fork() -> SyscallResult {
    syscall!(Fork)
}

wrap_c!(set_fs(addr: u64));
pub fn set_fs(addr: u64) -> SyscallResult {
    syscall!(SetFs, addr)
}

wrap_c!(get_fs());
pub fn get_fs() -> SyscallResult {
    syscall!(GetFs)
}

wrap_c!(set_gs(addr: u64));
pub fn set_gs(addr: u64) -> SyscallResult {
    syscall!(SetGs, addr)
}

wrap_c!(allocate_mem_pages(pages: u64, flags: u64));
fn allocate_mem_pages(pages: u64, flags: u64) -> SyscallResult {
    syscall!(AllocateMem, pages, flags)
}

wrap_c!(allocate_mem(len: u64, flags: u64));
pub fn allocate_mem(len: u64, flags: u64) -> SyscallResult {
    allocate_mem_pages((len + 4095) / 4096, flags)
}

wrap_c!(get_process_id());
pub fn get_process_id() -> SyscallResult {
    syscall!(GetProcessID)
}

wrap_c!(get_thread_id());
pub fn get_thread_id() -> SyscallResult {
    // TODO not yet implemented
    get_process_id()
}

pub fn execve(path: &str) -> SyscallResult {
    syscall!(Execve, path.as_bytes().as_ptr() as u64)
}
