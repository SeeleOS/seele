use crate::{
    multitasking::thread::THREAD_MANAGER,
    s_println,
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
        let mut manager = THREAD_MANAGER.get().unwrap().lock();

        manager.mark_current_as_zombie();

        drop(manager);
        s_println!("exit called wtf");
        Ok(0)
    }
}
