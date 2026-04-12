use crate::object::{ObjectResult, error::ObjectError};
use seele_sys::abi::{
    framebuffer::FramebufferInfo,
    object::{ConfigCommand, TerminalInfo},
};

use crate::terminal::linux_kd::{LinuxKbEntry, LinuxVtMode, LinuxVtStat};

pub enum ConfigurateRequest {
    GetTerminalInfo(*mut TerminalInfo),
    SetTerminalInfo(*const TerminalInfo),
    GetFramebufferInfo(*mut FramebufferInfo),
    FbTakeControl,
    FbRelease,
    TermSetActiveGroup(u64),
    TermGetActiveGroup,
    LinuxKdGetKeyboardMode(*mut u32),
    LinuxKdSetKeyboardMode(u32),
    LinuxKdGetKeyboardType(*mut u8),
    LinuxKdGetKeyboardEntry(*mut LinuxKbEntry),
    LinuxKdGetDisplayMode(*mut u32),
    LinuxKdSetDisplayMode(u32),
    LinuxVtOpenQuery(*mut u32),
    LinuxVtGetMode(*mut LinuxVtMode),
    LinuxVtGetState(*mut LinuxVtStat),
    LinuxVtSetMode(*const LinuxVtMode),
    LinuxVtActivate(u32),
    LinuxVtWaitActive(u32),
    LinuxVtRelDisp(u32),
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> ObjectResult<Self> {
        Ok(match request {
            0x4B44 => Self::LinuxKdGetKeyboardMode(ptr as *mut u32),
            0x4B45 => Self::LinuxKdSetKeyboardMode(ptr as u32),
            0x4B33 => Self::LinuxKdGetKeyboardType(ptr as *mut u8),
            0x4B46 => Self::LinuxKdGetKeyboardEntry(ptr as *mut LinuxKbEntry),
            0x4B3B => Self::LinuxKdGetDisplayMode(ptr as *mut u32),
            0x4B3A => Self::LinuxKdSetDisplayMode(ptr as u32),
            0x5600 => Self::LinuxVtOpenQuery(ptr as *mut u32),
            0x5601 => Self::LinuxVtGetMode(ptr as *mut LinuxVtMode),
            0x5603 => Self::LinuxVtGetState(ptr as *mut LinuxVtStat),
            0x5602 => Self::LinuxVtSetMode(ptr as *const LinuxVtMode),
            0x5606 => Self::LinuxVtActivate(ptr as u32),
            0x5607 => Self::LinuxVtWaitActive(ptr as u32),
            0x5605 => Self::LinuxVtRelDisp(ptr as u32),
            _ => match ConfigCommand::try_from(request).map_err(|_| ObjectError::InvalidRequest)? {
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
        })
    }
}
