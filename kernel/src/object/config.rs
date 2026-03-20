use crate::{
    object::{Object, ObjectResult, error::ObjectError},
    terminal::object_config::TerminalInfo,
};

pub enum ConfigurateRequest {
    GetTerminalInfo(*mut TerminalInfo),
    SetTerminalInfo(*const TerminalInfo),
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> ObjectResult<Self> {
        match request {
            0 => Ok(Self::GetTerminalInfo(ptr as *mut TerminalInfo)),
            1 => Ok(Self::SetTerminalInfo(ptr as *const TerminalInfo)),
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}
