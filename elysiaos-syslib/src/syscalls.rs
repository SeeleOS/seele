use crate::{errors::SyscallError, numbers::SyscallNumber, syscall, utils::SyscallResult};

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

pub fn exit() -> SyscallResult {
    syscall!(Exit)
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

pub fn get_thread_id() -> SyscallResult {
    // TODO not yet implemented
    get_process_id()
}
