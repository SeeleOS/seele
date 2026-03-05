use core::str::from_utf8;

use alloc::slice;

use crate::{
    filesystem::path::Path,
    multitasking::MANAGER,
    systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo},
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

        buf.copy_from_slice(
            process
                .lock()
                .current_directory
                .clone()
                .as_string()
                .as_bytes(),
        );

        Ok(arg1 as usize)
    }
}
