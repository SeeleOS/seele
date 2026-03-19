use crate::systemcall::error::SyscallError;

pub enum Command {
    SetFlags,
    GetFlags,
}

impl Command {
    pub fn new(val: u64) -> Result<Self, SyscallError> {
        match val {
            0 => Ok(Self::SetFlags),
            1 => Ok(Self::GetFlags),
            _ => Err(SyscallError::InvalidArguments),
        }
    }
}
