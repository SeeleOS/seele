use x86_64::registers::model_specific::FsBase;

use crate::systemcall::{
    error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo,
};

pub struct GetFSImpl;

impl SyscallImpl for GetFSImpl {
    const ENTRY: SyscallNo = SyscallNo::GetFs;
    fn handle_call(
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, SyscallError> {
        Ok(FsBase::read().as_u64() as usize)
    }
}
