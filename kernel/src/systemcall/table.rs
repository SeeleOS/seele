use crate::register_syscalls;
use crate::systemcall::implementations::*;
use crate::systemcall::utils::SyscallImpl;
use crate::systemcall::utils::SyscallResult;

type SyscallHandler = fn(u64, u64, u64, u64, u64, u64) -> SyscallResult;

pub static SYSCALL_TABLE: [Option<SyscallHandler>; 1500] = {
    let mut table = [None; 1500];

    register_syscalls!(
        table,
        Read,
        Write,
        OpenAt,
        Close,
        Fstat,
        Lseek,
        Mmap,
        Mprotect,
        Munmap,
        RtSigaction,
        RtSigprocmask,
        RtSigreturn,
        Ioctl,
        Dup,
        Nanosleep,
        Socket,
        Connect,
        Accept,
        Recvmsg,
        Shutdown,
        Bind,
        Listen,
        Getsockname,
        Getpeername,
        Getsockopt,
        Clone,
        Fork,
        Execve,
        Exit,
        Wait4,
        Kill,
        Uname,
        Fcntl,
        Getdents,
        Getcwd,
        Chdir,
        Readlink,
        Getpid,
        Getppid,
        Setpgid,
        Getpgid,
        Setsid,
        ArchPrctl,
        Gettid,
        Futex,
        EpollWait,
        EpollCtl,
        ClockGettime,
        MkdirAt,
        Newfstatat,
        UnlinkAt,
        RenameAt,
        LinkAt,
        ReadlinkAt,
        EpollPwait,
        Dup3,
        EpollCreate1,
        RenameAt2,
        TimerCreate,
        TimerSettime,
        TimerGettime,
        TimerGetoverrun,
        TimerDelete
    );

    table
};
