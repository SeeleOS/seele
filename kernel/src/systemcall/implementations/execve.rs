use core::str::from_utf8;

use alloc::{string::String, vec::Vec};
use vte::Parser;

use crate::{
    define_syscall,
    filesystem::path::Path,
    misc::others::from_cstr,
    multitasking::{MANAGER, process::execve::execve},
    systemcall::{error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

define_syscall!(Execve, |path_str: String| {
    let path = Path::new(path_str.as_str());

    execve(path, Vec::new())?;

    Ok(0)
});
