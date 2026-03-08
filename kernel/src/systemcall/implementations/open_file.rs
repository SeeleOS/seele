use crate::systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo};

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
        unimplemented!()
    }
}
