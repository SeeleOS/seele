use crate::register_syscalls;
use crate::systemcall::implementations::*;
use crate::systemcall::utils::SyscallImpl;
use crate::systemcall::utils::SyscallResult;

type SyscallHandler = fn(u64, u64, u64, u64, u64, u64) -> SyscallResult;

pub static SYSCALL_TABLE: [Option<SyscallHandler>; 1500] = {
    let mut table = [None; 1500];

    register_syscalls!(
        table,
        SetFs,
        GetFs,
        GetProcessID,
        GetThreadID,
        SetGs,
        GetCurrentDirectory,
        ChangeDirectory,
        RemoveObject,
        Fork,
        Execve,
        FutexWait,
        FutexWake,
        WriteObject,
        ReadObject,
        AllocateMem,
        FileInfo,
        OpenFile,
        Exit,
        ConfigurateObject,
        WaitForProcessExit,
        GetDirectoryContents,
        GetProcessParentID,
        ControlObject,
        CreatePoller,
        PollerAdd,
        PollerRemove,
        PollerWait,
        CloneObject,
        CloneObjectTo,
        RegisterSignalAction,
        SendSignal,
        UpdateMemPerms,
        DeallocateMem,
        BlockSignals,
        UnblockSignals,
        SetBlockedSignals,
        SigHandlerReturn,
        GetSystemInfo,
        GetCurrentTime,
        TimeSinceBoot,
        OpenDevice,
        MmapObject,
        Socket,
        SocketBind,
        SocketListen,
        SocketConnect,
        SocketAccept,
        SocketGetSockOpt,
        DeleteFile,
        LinkFile,
        Sleep,
        CreateDirectory,
        SeekObject,
        ThreadClone,
        LinkFile,
        SendSignalGroup
    );

    table
};
