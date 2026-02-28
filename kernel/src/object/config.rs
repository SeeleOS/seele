use core::fmt::Debug;

use alloc::sync::Arc;

use crate::{
    graphics::object_config::{TerminalInfo, WindowSizeInfo},
    multitasking::MANAGER,
    object::{Object, ObjectResult, error::ObjectError},
};

pub enum ConfigurateRequest {
    GetWindowSize(*mut WindowSizeInfo),
    GetTerminalInfo(*mut TerminalInfo),
    Unknown(u64, u64),
}

impl ConfigurateRequest {
    pub fn new(request: u64, ptr: u64) -> Self {
        match request {
            0x5401 => Self::GetTerminalInfo(ptr as *mut TerminalInfo),
            0x5413 => Self::GetWindowSize(ptr as *mut WindowSizeInfo),
            _ => Self::Unknown(request, ptr),
        }
    }
}

pub trait Configuratable: Object {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize>;
}
