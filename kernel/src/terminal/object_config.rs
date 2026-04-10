use crate::{
    misc::usercopy::{read_user_value, write_user_value},
    object::{
        config::ConfigurateRequest, error::ObjectError, misc::ObjectResult, traits::Configuratable,
    },
    terminal::TerminalObject,
};

impl Configuratable for TerminalObject {
    fn configure(&self, request: crate::object::config::ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::GetTerminalInfo(term_info) => {
                if !write_user_value(term_info, *self.info.lock()) {
                    return Err(ObjectError::InvalidArguments);
                }
            }
            ConfigurateRequest::SetTerminalInfo(term_info) => {
                let new_info = read_user_value(term_info).ok_or(ObjectError::InvalidArguments)?;
                *self.info.lock() = new_info;
            }
            _ => return Err(ObjectError::InvalidArguments),
        }
        Ok(0)
    }
}
