use crate::object::{ObjectResult, error::ObjectError};
use seele_sys::abi::{
    framebuffer::FramebufferInfo,
    object::{ConfigCommand, TerminalInfo},
};

pub enum ConfigurateRequest {
    GetTerminalInfo(*mut TerminalInfo),
    SetTerminalInfo(*const TerminalInfo),
    GetFramebufferInfo(*mut FramebufferInfo),
    FbTakeControl,
    FbRelease,
    TermSetActiveGroup(u64),
    TermGetActiveGroup,
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> ObjectResult<Self> {
        Ok(
            match ConfigCommand::try_from(request).map_err(|_| ObjectError::InvalidRequest)? {
                ConfigCommand::GetTerminalInfo => Self::GetTerminalInfo(ptr as *mut TerminalInfo),
                ConfigCommand::SetTerminalInfo => Self::SetTerminalInfo(ptr as *const TerminalInfo),
                ConfigCommand::GetFramebufferInfo => {
                    Self::GetFramebufferInfo(ptr as *mut FramebufferInfo)
                }
                ConfigCommand::FbTakeControl => Self::FbTakeControl,
                ConfigCommand::FbRelease => Self::FbRelease,
                ConfigCommand::TermSetActiveGroup => Self::TermSetActiveGroup(ptr),
                ConfigCommand::TermGetActiveGroup => Self::TermGetActiveGroup,
            },
        )
    }
}
