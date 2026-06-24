#![allow(unused_braces)]

use alloc::{string::String, vec::Vec};
use x86_64::VirtAddr;

use crate::{
    filesystem::vfs_traits::Whence,
    memory::protection::Protection,
    misc::{
        c_types::{CString, CVec},
        others::KernelFrom,
        timer::ClockId,
    },
    object::{
        linux_anon::{EventFdFlags, SignalfdFlags},
        misc::{ObjectRef, get_object_current_process},
    },
    polling::event::PollableEvent,
    process::{
        ProcessRef,
        group::ProcessGroupID,
        misc::{ProcessID, get_process_with_pid},
    },
    signal::{Signal, Signals},
    systemcall::implementations::{
        AtFlags, ClockNanosleepFlags, CloseRangeFlags, DupFlags, EpollCreateFlags, FallocateFlags,
        FsMountFlags, FsOpenFlags, GetMempolicyFlags, GetRandomFlags, InotifyInitFlags,
        LinuxIoprioWho, LinuxSchedPolicy, MemfdFlags, MmapFlags, MoveMountFlags, MremapFlags,
        MsyncFlags, OpenFlags, OpenTreeAttrFlags, OpenTreeFlags, PipeFlags, PollEvents, RseqFlags,
        SetnsFlags, TimerFdFlags, TimerSetTimeFlags, UmountFlags, Wait4Options, WaitidOptions,
        XattrFlags,
    },
    systemcall::utils::{SyscallError, SyscallResult, invalid_syscall_flag_error},
};

macro_rules! add_syscall_arg_type {
    ($($type: ty),*) => {
        $(
            impl SyscallArg for $type {
                fn from_u64(val: u64) -> SyscallResult<Self> {
                    Ok(val as $type)
                }
            }
        )*
    };

    ($type: ty, $name: ident, $convert: expr) => {
        impl SyscallArg for $type {
            fn from_u64($name: u64) -> SyscallResult<Self>
            where
                Self: Sized,
            {
                $convert
            }
        }
    }
}

macro_rules! add_syscall_arg_flags_type {
    ($($type:ty, $raw:ty),* $(,)?) => {
        $(
            add_syscall_arg_type!($type, val, {
                <$type>::from_bits(val as $raw)
                    .ok_or_else(|| invalid_syscall_flag_error::<$type>(val))
            });
        )*
    };
}

macro_rules! add_syscall_arg_flags_retain_type {
    ($($type:ty, $raw:ty),* $(,)?) => {
        $(
            add_syscall_arg_type!($type, val, {
                Ok(<$type>::from_bits_retain(val as $raw))
            });
        )*
    };
}

pub trait SyscallArg {
    fn from_u64(val: u64) -> SyscallResult<Self>
    where
        Self: Sized;
}

impl<T> SyscallArg for *const T {
    fn from_u64(val: u64) -> SyscallResult<Self> {
        Ok(val as *const T)
    }
}

impl<T> SyscallArg for *mut T {
    fn from_u64(val: u64) -> SyscallResult<Self> {
        Ok(val as *mut T)
    }
}

add_syscall_arg_type!(u32, usize, u64, i32, i64);

add_syscall_arg_type!(Vec<String>, val, {
    Vec::k_from(val as CVec<CString>).map_err(|err| err.as_syscall_error())
});

add_syscall_arg_type!(String, val, {
    if val == 0 {
        Err(SyscallError::BadAddress)
    } else {
        String::k_from(val as *const u8).map_err(|err| err.as_syscall_error())
    }
});

add_syscall_arg_type!(bool, val, { Ok(val != 0) });

add_syscall_arg_type!(ProcessID, val, { Ok(ProcessID(val)) });

add_syscall_arg_type!(ObjectRef, val, {
    get_object_current_process(val).map_err(Into::into)
});

add_syscall_arg_type!(PollableEvent, val, { Ok(PollableEvent::from(val)) });

add_syscall_arg_type!(Signal, val, {
    Signal::try_from(val).map_err(|_| SyscallError::InvalidArguments)
});

add_syscall_arg_type!(ProcessRef, val, { get_process_with_pid(ProcessID(val)) });

add_syscall_arg_type!(VirtAddr, val, { Ok(VirtAddr::new(val)) });

add_syscall_arg_type!(Whence, val, {
    Whence::try_from(val).map_err(|_| SyscallError::InvalidArguments)
});

add_syscall_arg_type!(ProcessGroupID, val, { Ok(ProcessGroupID(val)) });

add_syscall_arg_type!(ClockId, val, {
    ClockId::try_from(val).map_err(|_| SyscallError::InvalidArguments)
});

add_syscall_arg_type!(LinuxSchedPolicy, val, {
    LinuxSchedPolicy::try_from(val as i32).map_err(|_| SyscallError::InvalidArguments)
});

add_syscall_arg_type!(LinuxIoprioWho, val, {
    LinuxIoprioWho::try_from(val as i32).map_err(|_| SyscallError::InvalidArguments)
});

add_syscall_arg_flags_type!(
    Signals,
    u64,
    Protection,
    u64,
    PollEvents,
    i16,
    EpollCreateFlags,
    i32,
    InotifyInitFlags,
    i32,
    EventFdFlags,
    i32,
    TimerFdFlags,
    i32,
    TimerSetTimeFlags,
    i32,
    ClockNanosleepFlags,
    i32,
    SignalfdFlags,
    i32,
    PipeFlags,
    i32,
    MsyncFlags,
    i32,
    FallocateFlags,
    i32,
    UmountFlags,
    i32,
    FsOpenFlags,
    u32,
    FsMountFlags,
    u32,
    MoveMountFlags,
    u32,
    GetMempolicyFlags,
    u64,
    OpenTreeFlags,
    u32,
    OpenTreeAttrFlags,
    u64,
    MremapFlags,
    u64,
    RseqFlags,
    u32,
    GetRandomFlags,
    u32,
    SetnsFlags,
    u32,
    Wait4Options,
    i32,
    WaitidOptions,
    i32,
    DupFlags,
    i32,
    MemfdFlags,
    u32,
);

add_syscall_arg_flags_retain_type!(
    MmapFlags,
    i32,
    AtFlags,
    i32,
    CloseRangeFlags,
    u32,
    OpenFlags,
    i32,
    XattrFlags,
    u32
);
