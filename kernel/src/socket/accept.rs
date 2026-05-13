use alloc::sync::Arc;

use super::{
    SocketError, SocketResult, UnixSocketObject, UnixSocketState, self_ref::object_ref,
    wait::wait_for_object_event,
};
use crate::{polling::event::PollableEvent, process::manager::get_current_process};

impl UnixSocketObject {
    pub fn accept(self: &Arc<Self>) -> SocketResult<usize> {
        loop {
            let listener = match &*self.state.lock() {
                UnixSocketState::Listener(listener) => listener.clone(),
                _ => return Err(SocketError::InvalidArguments),
            };

            if let Some(socket) = listener.pending.lock().pop_front() {
                return Ok(get_current_process().lock().push_object(socket));
            }

            if self.is_nonblocking() {
                return Err(SocketError::TryAgain);
            }

            if let Some(object) = object_ref(&self.self_ref) {
                let object_ref = object as crate::object::misc::ObjectRef;
                wait_for_object_event(object_ref, PollableEvent::CanBeRead);
            } else if !listener.pending.lock().is_empty() {
                continue;
            }
        }
    }
}
