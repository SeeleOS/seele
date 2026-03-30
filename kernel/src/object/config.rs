use crate::{
    object::{ObjectResult, error::ObjectError},
};
use seele_sys::syscalls::object::{ConfigCommand, TerminalInfo};

pub enum ConfigurateRequest {
    GetTerminalInfo(*mut TerminalInfo),
    SetTerminalInfo(*const TerminalInfo),
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> ObjectResult<Self> {
        match ConfigCommand::from_raw_u64(request) {
            Some(ConfigCommand::GetTerminalInfo) => Ok(Self::GetTerminalInfo(ptr as *mut TerminalInfo)),
            Some(ConfigCommand::SetTerminalInfo) => Ok(Self::SetTerminalInfo(ptr as *const TerminalInfo)),
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}
