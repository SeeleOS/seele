use core::ops::Deref;

pub use seele_sys::abi::object::ControlCommand;
use seele_sys::abi::object::ObjectFlags;
use seele_sys::{SyscallResult, errors::SyscallError};
use x86_64::instructions::interrupts::are_enabled;

use crate::object::misc::ObjectRef;
use crate::process::misc::with_current_process;
use crate::terminal::misc::clear;

pub fn control_object(object: ObjectRef, command: u64, arg: u64) -> SyscallResult {
    match ControlCommand::from_u64(command)? {
        ControlCommand::SetFlags => object
            .set_flags(ObjectFlags::from_bits(arg).ok_or(SyscallError::InvalidArguments)?)
            .map(|_| 0usize)
            .map_err(Into::into),
        ControlCommand::GetFlags => object
            .get_flags()
            .map_err(Into::into)
            .map(|f| f.bits() as usize),
        ControlCommand::CloneWithMin => with_current_process(|process| {
            process
                .clone_object_with_min(object, arg as usize)
                .map_err(Into::into)
                .map(Into::into)
        }),
        ControlCommand::CloneWithMinCloseOnExecve => with_current_process(|process| {
            process
                .clone_object_with_min(object, arg as usize)
                .map(Into::into)
                .map_err(Into::into)
        }),
    }
}
