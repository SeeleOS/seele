use crate::object::FileFlags;
use seele_sys::{SyscallResult, errors::SyscallError};

use crate::object::misc::ObjectRef;
use crate::process::misc::with_current_process;

const F_DUPFD: u64 = 0;
const F_GETFD: u64 = 1;
const F_SETFD: u64 = 2;
const F_GETFL: u64 = 3;
const F_SETFL: u64 = 4;
const F_DUPFD_CLOEXEC: u64 = 1030;

pub fn control_object(object: ObjectRef, command: u64, arg: u64) -> SyscallResult {
    match command {
        F_SETFL => object
            .set_flags(FileFlags::from_bits(arg).ok_or(SyscallError::InvalidArguments)?)
            .map(|_| 0usize)
            .map_err(Into::into),
        F_GETFL => object
            .get_flags()
            .map_err(Into::into)
            .map(|f| f.bits() as usize),
        F_DUPFD => with_current_process(|process| {
            process
                .clone_object_with_min(object, arg as usize)
                .map_err(Into::into)
        }),
        F_DUPFD_CLOEXEC => with_current_process(|process| {
            process
                .clone_object_with_min(object, arg as usize)
                .map_err(Into::into)
        }),
        F_SETFD | F_GETFD => Ok(0),
        _ => Err(SyscallError::InvalidArguments),
    }
}
