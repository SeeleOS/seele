use core::ptr::{read, read_volatile, write_volatile};

use fatfs::FsStatusFlags;

use crate::{
    object::{config::ConfigurateRequest, misc::ObjectResult, traits::Configuratable},
    terminal::{TerminalObject, term_trait::TerminalSize},
};

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TerminalInfo {
    pub rows: u64,
    pub cols: u64,
    pub echo: bool,
    /// Whether the kernel tty should handle canonical line discipline semantics
    /// such as line buffering, erase, and newline submission.
    pub canonical: bool,
    pub echo_newline: bool,
    pub echo_delete: bool,
}

impl TerminalInfo {
    pub fn new(size: TerminalSize) -> Self {
        Self {
            rows: size.rows,
            cols: size.cols,
            echo: true,
            echo_newline: true,
            echo_delete: true,
            canonical: true,
        }
    }
}

impl Configuratable for TerminalObject {
    fn configure(&self, request: crate::object::config::ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::GetTerminalInfo(term_info) => unsafe {
                write_volatile(term_info, *self.info.lock());
            },
            ConfigurateRequest::SetTerminalInfo(term_info) => unsafe {
                let new_info = read_volatile(term_info);

                *self.info.lock() = new_info;
            },
            _ => {}
        }
        Ok(0)
    }
}
