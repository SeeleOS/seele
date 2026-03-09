use crate::{errors::SyscallError, syscall, utils::SyscallResult};

pub fn change_dir(dir: *const i8, len: u64) -> SyscallResult {
    syscall!(ChangeDirectory, dir as u64, len)
}

pub fn get_current_directory(buf: &mut [u8]) -> SyscallResult {
    syscall!(GetCurrentDirectory, buf.as_ptr() as u64, buf.len() as u64)
}

pub fn open_file(path: *const i8) -> SyscallResult {
    syscall!(OpenFile, path as u64)
}

pub fn file_info(
    from_current_dir: bool,
    path_ptr: *const i8,
    stat_ptr: *const u8,
) -> SyscallResult {
    syscall!(
        FileInfo,
        from_current_dir as u64,
        path_ptr as u64,
        stat_ptr as u64
    )
}
