pub use seele_sys::syscalls::object::ControlCommand;
use seele_sys::syscalls::object::ObjectFlags;
use seele_sys::{SyscallResult, errors::SyscallError};
use x86_64::instructions::interrupts::are_enabled;

pub enum ControlRequest {
    GetFlags,
    SetFlags(ObjectFlags),
}

impl ControlRequest {
    pub fn new(command: u64, arg: u64) -> SyscallResult<Self> {
        Ok(match ControlCommand::from_u64(command)? {
            ControlCommand::SetFlags => {
                Self::SetFlags(ObjectFlags::from_bits(arg).ok_or(SyscallError::InvalidArguments)?)
            }
            ControlCommand::GetFlags => Self::GetFlags,
        })
    }
}
