use core::ptr::{read_volatile, write_volatile};

use crate::{
    object::{
        config::ConfigurateRequest, error::ObjectError, misc::ObjectResult, traits::Configuratable,
    },
    terminal::{
        TerminalObject,
        linux_kd::handle_kd_request,
        linux_vt::handle_vt_request,
    },
};

impl Configuratable for TerminalObject {
    fn configure(&self, request: crate::object::config::ConfigurateRequest) -> ObjectResult<isize> {
        if let Some(result) = handle_kd_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        if let Some(result) = handle_vt_request(&self.linux_console, &request)? {
            return Ok(result);
        }

        match request {
            ConfigurateRequest::GetTerminalInfo(term_info) => unsafe {
                write_volatile(term_info, *self.info.lock());
            },
            ConfigurateRequest::SetTerminalInfo(term_info) => unsafe {
                let new_info = read_volatile(term_info);

                *self.info.lock() = new_info;
            },
            _ => return Err(ObjectError::InvalidArguments),
        }
        Ok(0)
    }
}
