use crate::{
    register_syscall,
    systemcall::{
        error::SyscallError,
        implementations::{
            allocate_mem::AllocMemImpl,
            exit::ExitImpl,
            futex::{FutexWaitImpl, FutexWakeImpl},
            get_fs::GetFSImpl,
            get_process_id::GetPIDImpl,
            get_thread_id::GetTIDImpl,
            print::PrintImpl,
            set_fs::SetFSImpl,
            set_gs::SetGSImpl,
            utils::SyscallImpl,
        },
        syscall_no::SyscallNo,
    },
};

type SyscallHandler = fn(u64, u64, u64, u64, u64, u64) -> Result<usize, SyscallError>;

pub static SYSCALL_TABLE: [Option<SyscallHandler>; 512] = {
    let mut table = [None; 512];

    // 编译时初始化表
    register_syscall!(table, SyscallNo::Print, PrintImpl);
    register_syscall!(table, SyscallNo::SetGs, SetGSImpl);
    register_syscall!(table, SyscallNo::SetFs, SetFSImpl);
    register_syscall!(table, SyscallNo::GetFs, GetFSImpl);
    register_syscall!(table, SyscallNo::AllocateMem, AllocMemImpl);
    register_syscall!(table, SyscallNo::GetProcessID, GetPIDImpl);
    register_syscall!(table, SyscallNo::GetThreadID, GetTIDImpl);
    register_syscall!(table, SyscallNo::FutexWait, FutexWaitImpl);
    register_syscall!(table, SyscallNo::FutexWake, FutexWakeImpl);
    register_syscall!(table, SyscallNo::Exit, ExitImpl);

    table
};
