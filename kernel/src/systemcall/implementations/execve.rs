use core::str::from_utf8;

use alloc::{slice, vec::Vec};
use vte::Parser;

use crate::{
    filesystem::path::Path,
    misc::others::from_cstr,
    multitasking::{MANAGER, process::execve::execve},
    systemcall::{error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct ExecveImpl;

impl SyscallImpl for ExecveImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::Execve;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let path_str = unsafe { from_cstr(arg1 as *const u8)? };

        let path = Path::new(path_str.as_str());

        execve(path, Vec::new())?;

        Ok(0)
    }
}
