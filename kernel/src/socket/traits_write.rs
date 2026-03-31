use alloc::sync::Weak;

use super::{UnixSocketObject, UnixSocketState, wake_io, wake_pollers};
use crate::{
    object::{error::ObjectError, traits::Writable},
    polling::event::PollableEvent,
};

impl Writable for UnixSocketObject {
    fn write(&self, buffer: &[u8]) -> Result<usize, ObjectError> {
        let stream = match &*self.state.lock() {
            UnixSocketState::Stream(stream) => stream.clone(),
            _ => return Err(ObjectError::InvalidArguments),
        };

        let peer = stream
            .peer
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or(ObjectError::BrokenPipe)?;
        peer.recv_buf.lock().extend(buffer.iter().copied());

        if let Some(owner) = peer.owner.lock().as_ref().and_then(Weak::upgrade) {
            wake_pollers(&owner, PollableEvent::CanBeRead);
        }
        wake_io();
        Ok(buffer.len())
    }
}
