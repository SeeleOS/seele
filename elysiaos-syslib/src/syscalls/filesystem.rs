use crate::{errors::SyscallError, syscall, utils::SyscallResult};

pub fn change_dir(dir: *const i8, len: u64) -> SyscallResult {
    syscall!(ChangeDirectory, dir as u64, len)
}

pub fn get_current_directory(buf: &mut [u8]) -> SyscallResult {
    syscall!(GetCurrentDirectory, buf.as_ptr() as u64, buf.len() as u64)
}

pub fn open_file(path: &str) -> SyscallResult {
    syscall!(OpenFile, path.as_ptr() as u64)
}
