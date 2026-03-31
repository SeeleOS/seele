use alloc::sync::Arc;

use super::{UnixSocketObject, UnixSocketState};
use crate::{
    object::{error::ObjectError, misc::ObjectResult},
    process::manager::get_current_process,
    thread::yielding::{BlockType, WakeType, block_current},
};

impl UnixSocketObject {
    pub fn accept(self: &Arc<Self>) -> ObjectResult<usize> {
        loop {
            let listener = match &*self.state.lock() {
                UnixSocketState::Listener(listener) => listener.clone(),
                _ => return Err(ObjectError::InvalidArguments),
            };

            if let Some(socket) = listener.pending.lock().pop_front() {
                return Ok(get_current_process().lock().push_object(socket));
            }

            if self.is_nonblocking() {
                return Err(ObjectError::TryAgain);
            }

            block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });
        }
    }
}
