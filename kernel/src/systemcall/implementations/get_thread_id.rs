
use crate::systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo};

pub struct GetTIDImpl;

impl SyscallImpl for GetTIDImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::GetThreadID;

    fn handle_call(
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        unimplemented!()
    }
}
