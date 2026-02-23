use crate::{
    multitasking::{MANAGER, exit_handling::exit_handler, scheduling::run_next_zombie},
    systemcall::{implementations::utils::SyscallImpl, syscall_no::SyscallNo},
};

pub struct ExitImpl;

impl SyscallImpl for ExitImpl {
    const ENTRY: crate::systemcall::syscall_no::SyscallNo = SyscallNo::Exit;

    fn handle_call(
        arg1: u64,
        arg2: u64,
        arg3: u64,
        arg4: u64,
        arg5: u64,
        arg6: u64,
    ) -> Result<usize, crate::systemcall::error::SyscallError> {
        // TODO: release the memory when exitting
        let mut manager = MANAGER.lock();
        if let Some(pid) = manager.current {
            manager.zombies.push(pid);
        }

        run_next_zombie();
        Ok(0)
    }
}
