use crate::{errors::SyscallError, numbers::SyscallNumber, syscall, utils::SyscallResult, wrap_c};

pub mod futex;
pub mod object;

#[inline(always)]
pub fn print(value: &str) -> SyscallResult {
    let msg = value.as_bytes();
    let buf = msg.as_ptr();
    let count = msg.len();

    syscall!(Print, buf as u64, count as u64)
}

#[inline]
pub fn print_buf(buf: &[u8], len: u64) -> SyscallResult {
    let buf = buf.as_ptr();

    syscall!(Print, buf as u64, len)
}

wrap_c!(exit());
pub fn exit() -> SyscallResult {
    syscall!(Exit)
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
