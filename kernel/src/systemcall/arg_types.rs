
use alloc::{string::String, vec::Vec};

use crate::{
    filesystem::info::LinuxStat,
    misc::{
        c_types::{CString, CVec},
        others::KernelFrom,
    },
    multitasking::process::misc::ProcessID,
    object::misc::{ObjectRef, get_object_current_process},
    polling::event::PollableEvent,
    systemcall::{error::SyscallError, implementations::PollResult},
};

macro_rules! add_syscall_arg_type {
    ($($type: ty),*) => {
        $(
            impl SyscallArg for $type {
                fn from_u64(val: u64) -> Result<Self, SyscallError> {
                    Ok(val as $type)
                }
            }
        )*
    };
}

pub trait SyscallArg {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized;
}

add_syscall_arg_type!(
    u32,
    usize,
    *mut LinuxStat,
    u64,
    *mut u8,
    *mut u64,
    *mut PollResult
);

impl SyscallArg for Vec<String> {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized,
    {
        Ok(Vec::k_from(val as CVec<CString>)?)
    }
}

impl SyscallArg for String {
    fn from_u64(val: u64) -> Result<Self, SyscallError> {
        Ok(String::k_from(val as *const u8)?)
    }
}

impl SyscallArg for bool {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized,
    {
        Ok(val != 0)
    }
}

impl SyscallArg for ProcessID {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized,
    {
        Ok(ProcessID(val))
    }
}

impl SyscallArg for ObjectRef {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized,
    {
        get_object_current_process(val).map_err(Into::into)
    }
}

impl SyscallArg for PollableEvent {
    fn from_u64(val: u64) -> Result<Self, SyscallError>
    where
        Self: Sized,
    {
        Ok(PollableEvent::from(val))
    }
}
