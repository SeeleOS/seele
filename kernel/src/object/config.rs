use crate::object::{ObjectResult, error::ObjectError};
use seele_sys::abi::object::{ConfigCommand, TerminalInfo};

pub enum ConfigurateRequest {
    GetTerminalInfo(*mut TerminalInfo),
    SetTerminalInfo(*const TerminalInfo),
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> ObjectResult<Self> {
        Ok(
            match ConfigCommand::from_raw_u64(request).ok_or(ObjectError::InvalidRequest)? {
                ConfigCommand::GetTerminalInfo => Self::GetTerminalInfo(ptr as *mut TerminalInfo),
                ConfigCommand::SetTerminalInfo => Self::SetTerminalInfo(ptr as *const TerminalInfo),
            },
        )
    }
}
