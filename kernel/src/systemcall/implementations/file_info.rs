use core::str::from_utf8;

use alloc::{slice, string::ToString};
use bootloader_api::info;
use x86_64::structures::paging::PageTableFlags;

use crate::{
    filesystem::{info::LinuxStat, path::Path, vfs::VirtualFS},
    misc::others::from_cstr,
    multitasking::process::manager::current_process,
    println,
    systemcall::{error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct FileInfoImpl;

impl SyscallImpl for FileInfoImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::FileInfo;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        let path_str = unsafe { from_cstr(arg2 as *const u8)? };
        let path: Path;
        if path_str.starts_with('/') {
            path = Path::new(&path_str);
        } else {
            if arg1 == 1 {
                // start from current directory
                path = Path::new(
                    (current_process().lock().current_directory.1.clone() + &path_str).as_str(),
                );
            } else {
                return Err(SyscallError::other(
                    "Non-absolute paths are not supported yet",
                ));
            }
        }

        let info = VirtualFS.lock().file_info(path).unwrap();

        unsafe { (*(arg3 as *mut LinuxStat)) = info.as_linux() };

        Ok(0)
    }
}
