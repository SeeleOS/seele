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
        AtFlags, ClockNanosleepFlags, EpollCreateFlags, FsMountFlags, FsOpenFlags, GetRandomFlags,
        InotifyInitFlags, MmapFlags, MoveMountFlags, MremapFlags, OpenFlags, OpenTreeFlags,
        PipeFlags, PollEvents, RseqFlags, TimerFdFlags, TimerSetTimeFlags, UmountFlags, XattrFlags,
    },
    systemcall::utils::{SyscallError, SyscallResult},
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

add_syscall_arg_type!(Signals, val, {
    Signals::from_bits(val).ok_or(SyscallError::InvalidArguments)
});

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

add_syscall_arg_type!(Protection, val, {
    Protection::from_bits(val).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(Whence, val, {
    Whence::try_from(val).map_err(|_| SyscallError::InvalidArguments)
});

add_syscall_arg_type!(ProcessGroupID, val, { Ok(ProcessGroupID(val)) });

add_syscall_arg_type!(ClockId, val, {
    ClockId::try_from(val).map_err(|_| SyscallError::InvalidArguments)
});

add_syscall_arg_type!(PollEvents, val, {
    PollEvents::from_bits(val as i16).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(EpollCreateFlags, val, {
    EpollCreateFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(InotifyInitFlags, val, {
    InotifyInitFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(EventFdFlags, val, {
    EventFdFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(TimerFdFlags, val, {
    TimerFdFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(TimerSetTimeFlags, val, {
    TimerSetTimeFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(ClockNanosleepFlags, val, {
    ClockNanosleepFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(SignalfdFlags, val, {
    SignalfdFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(PipeFlags, val, {
    PipeFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(AtFlags, val, {
    AtFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(OpenFlags, val, {
    OpenFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(XattrFlags, val, {
    XattrFlags::from_bits(val as u32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(UmountFlags, val, {
    UmountFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(FsOpenFlags, val, {
    FsOpenFlags::from_bits(val as u32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(FsMountFlags, val, {
    FsMountFlags::from_bits(val as u32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(MoveMountFlags, val, {
    MoveMountFlags::from_bits(val as u32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(OpenTreeFlags, val, {
    OpenTreeFlags::from_bits(val as u32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(MmapFlags, val, {
    MmapFlags::from_bits(val as i32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(MremapFlags, val, {
    MremapFlags::from_bits(val).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(RseqFlags, val, {
    RseqFlags::from_bits(val as u32).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(GetRandomFlags, val, {
    GetRandomFlags::from_bits(val as u32).ok_or(SyscallError::InvalidArguments)
});
