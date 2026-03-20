use crate::{
    object::{Object, ObjectResult},
    terminal::object_config::TerminalInfo,
};

pub enum ConfigurateRequest {
    GetTerminalInfo(*mut TerminalInfo),
    SetTerminalInfo(*const TerminalInfo),
    Unknown(u64, u64),
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> Self {
        match request {
            0 => Self::GetTerminalInfo(ptr as *mut TerminalInfo),
            1 => Self::SetTerminalInfo(ptr as *const TerminalInfo),
            _ => Self::Unknown(request, ptr),
        }
    }
}
