use super::UnixSocketObject;
use crate::object::{
    control::ControlRequest,
    error::ObjectError,
    traits::Controllable,
};

impl Controllable for UnixSocketObject {
    fn control(&self, request: ControlRequest) -> Result<isize, ObjectError> {
        match request {
            ControlRequest::GetFlags => Ok(self.flags.lock().bits() as isize),
            ControlRequest::SetFlags(flags) => {
                *self.flags.lock() = flags;
                Ok(0)
            }
        }
    }
}
