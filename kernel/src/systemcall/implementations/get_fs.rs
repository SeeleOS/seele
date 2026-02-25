use x86_64::registers::model_specific::{FsBase, Msr};

use crate::systemcall::{
    error::SyscallError, implementations::utils::SyscallImpl, syscall_no::SyscallNo,
};

pub struct GetFSImpl;

impl SyscallImpl for GetFSImpl {
    const ENTRY: SyscallNo = SyscallNo::GetFs;
    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, SyscallError> {
        Ok(FsBase::read().as_u64() as usize)
    }
}
