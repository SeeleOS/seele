use x86_64::{
    VirtAddr,
    registers::model_specific::FsBase,
};

use crate::systemcall::{error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo};

pub struct SetFSImpl;

impl SyscallImpl for SetFSImpl {
    const ENTRY: SyscallNo = SyscallNo::SetFs;

    fn handle_call(
        arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, SyscallError> {
        FsBase::write(VirtAddr::new(arg1));
        Ok(0)
    }
}
