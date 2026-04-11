use alloc::{string::String, vec::Vec};
use futures_util::future::OkInto;
use seele_sys::{
    SyscallResult,
    abi::{
        object::SeekType,
        time::{TimeType, TimerNotifyStruct, TimerStateStruct},
    },
    errors::SyscallError,
    misc::SystemInfo,
    permission::Permissions,
    signal::Signals,
};
use x86_64::VirtAddr;

use crate::{
    define_syscall,
    filesystem::info::LinuxStat,
    misc::{
        c_types::{CString, CVec},
        error::AsSyscallError,
        others::KernelFrom,
    },
    object::misc::{ObjectRef, get_object_current_process},
    polling::event::PollableEvent,
    process::{
        ProcessRef,
        group::ProcessGroupID,
        manager::MANAGER,
        misc::{ProcessID, get_process_with_pid},
    },
    signal::{Signal, action::SignalAction},
    systemcall::implementations::PollResult,
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

add_syscall_arg_type!(
    u32,
    usize,
    *mut LinuxStat,
    *const TimerNotifyStruct,
    *mut u32,
    u64,
    *mut u8,
    *mut u64,
    *mut PollResult,
    i32,
    *const TimerStateStruct,
    *mut i32,
    *mut TimerStateStruct,
    i64,
    *mut SignalAction,
    *const SignalAction,
    *mut Signals,
    *mut SystemInfo
);

add_syscall_arg_type!(Signals, val, {
    Signals::from_bits(val).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(Vec<String>, val, {
    Vec::k_from(val as CVec<CString>).map_err(|err| err.as_syscall_error())
});

add_syscall_arg_type!(String, val, {
    String::k_from(val as *const u8).map_err(|err| err.as_syscall_error())
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

add_syscall_arg_type!(Permissions, val, {
    Permissions::from_bits(val).ok_or(SyscallError::InvalidArguments)
});

add_syscall_arg_type!(SeekType, val, {
    SeekType::try_from(val).map_err(|_| SyscallError::InvalidArguments)
});

add_syscall_arg_type!(ProcessGroupID, val, { Ok(ProcessGroupID(val)) });

add_syscall_arg_type!(TimeType, val, {
    TimeType::try_from(val).map_err(|_| SyscallError::InvalidArguments)
});
