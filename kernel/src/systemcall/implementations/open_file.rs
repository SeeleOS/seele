use alloc::{slice, sync::Arc};

use crate::{
    filesystem::{path::Path, vfs::VirtualFS},
    misc::{error::AsSyscallError, others::from_cstr},
    multitasking::MANAGER,
    systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct OpenFileImpl;

impl SyscallImpl for OpenFileImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::OpenFile;

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

        let object = Arc::new(VirtualFS.lock().open(path)?);

        let current_process = &mut MANAGER.lock().current.clone().unwrap();
        current_process.lock().objects.push(object);
        Ok(current_process.lock().objects.len() + 1)
    }
}
