use crate::{
    multitasking::{MANAGER, scheduling::run_next_zombie},
    systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct ExitImpl;

impl SyscallImpl for ExitImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::Exit;

    fn handle_call(
        _arg1: u64,
        _arg2: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        _arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        // TODO: release the memory when exitting
        // TODO: it seemed to also be broken, ill fix it later
        let mut manager = MANAGER.lock();
        if let Some(pid) = manager.current {
            manager.zombies.push(pid);
        }

        drop(manager);

        run_next_zombie();
        Ok(0)
    }
}
