use core::str::from_utf8;

use alloc::slice;

use crate::{
    filesystem::path::Path,
    multitasking::MANAGER,
    s_print,
    systemcall::{error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct ChangeDirImpl;

impl SyscallImpl for ChangeDirImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::ChangeDirectory;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let target = unsafe { slice::from_raw_parts(arg1 as *const u8, arg2 as usize) };
        let process = MANAGER.lock().current.clone().unwrap();

        process.lock().current_directory = Path::new(from_utf8(target).unwrap());

        Ok(0)
    }
}

pub struct GetDirImpl;

impl SyscallImpl for GetDirImpl {
    const ENTRY: SyscallNo = SyscallNo::GetCurrentDirectory;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let buf = unsafe { slice::from_raw_parts_mut(arg1 as *mut u8, arg2 as usize) };

        let process = MANAGER.lock().current.clone().unwrap();
        let path_str = process.lock().current_directory.clone().as_string().clone();
        let path_bytes = path_str.as_bytes();

        let path_len = path_bytes.len();

        if arg2 as usize > path_len {
            // only copy the needed part
            buf[..path_len].copy_from_slice(path_bytes);

            // add \0
            buf[path_len] = 0;
        } else {
            return Err(SyscallError::InvalidArguments);
        }

        Ok(arg1 as usize)
    }
}
