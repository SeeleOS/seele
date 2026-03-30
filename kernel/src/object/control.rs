pub use seele_sys::syscalls::object::ControlCommand;
use seele_sys::errors::SyscallError;
use seele_sys::syscalls::object::ObjectFlags;
use x86_64::instructions::interrupts::are_enabled;

pub enum ControlRequest {
    GetFlags,
    SetFlags(ObjectFlags),
}

impl ControlRequest {
    pub fn new(command: ControlCommand, arg: u64) -> Result<Self, SyscallError> {
        Ok(match command {
            ControlCommand::SetFlags => {
                Self::SetFlags(ObjectFlags::from_bits(arg).ok_or(SyscallError::InvalidArguments)?)
            }
            ControlCommand::GetFlags => Self::GetFlags,
        })
    }
}
