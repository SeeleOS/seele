use crate::{
    graphics::object_config::{TerminalInfo, WindowSizeInfo},
    object::{Object, ObjectResult},
};

pub enum ConfigurateRequest {
    GetWindowSize(*mut WindowSizeInfo),
    GetTerminalInfo(*mut TerminalInfo),
    SetTerminalInfo(*const TerminalInfo),
    Unknown(u64, u64),
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> Self {
        match request {
            0x5401 => Self::GetTerminalInfo(ptr as *mut TerminalInfo),
            0x5402 => Self::SetTerminalInfo(ptr as *const TerminalInfo),
            0x5413 => Self::GetWindowSize(ptr as *mut WindowSizeInfo),
            _ => Self::Unknown(request, ptr),
        }
    }
}
