
use crate::{
    multitasking::MANAGER,
    systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct GetPIDImpl;

impl SyscallImpl for GetPIDImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::GetProcessID;

    fn handle_call(
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        Ok(MANAGER
            .lock()
            .current
            .expect("Theres no current process. WHAT? HOW?")
            .0 as usize)
    }
}
