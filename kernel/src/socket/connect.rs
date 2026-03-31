use alloc::{string::String, sync::Arc};
use seele_sys::abi::object::ObjectFlags;
use spin::Mutex;

use super::{
    UNIX_SOCKET_REGISTRY, UnixSocketObject, UnixSocketState, UnixStreamInner, wake_io,
    wake_pollers,
};
use crate::{object::{error::ObjectError, misc::ObjectResult}, polling::event::PollableEvent};

impl UnixSocketObject {
    pub fn connect(self: &Arc<Self>, path: String) -> ObjectResult<()> {
        let listener = {
            let registry = UNIX_SOCKET_REGISTRY.lock();
            match registry.get(&path) {
                Some(Some(listener)) => listener.clone(),
                _ => return Err(ObjectError::ConnectionRefused),
            }
        };

        match &*self.state.lock() {
            UnixSocketState::Unbound => {}
            UnixSocketState::Stream(_) => return Err(ObjectError::IsConnected),
            _ => return Err(ObjectError::InvalidArguments),
        }

        let (client_stream, server_stream) = UnixStreamInner::pair();
        *client_stream.owner.lock() = Some(Arc::downgrade(self));
        let server_socket = Arc::new(Self {
            state: Mutex::new(UnixSocketState::Stream(server_stream.clone())),
            flags: Mutex::new(ObjectFlags::empty()),
        });
        *server_stream.owner.lock() = Some(Arc::downgrade(&server_socket));

        let mut pending = listener.pending.lock();
        if pending.len() >= listener.backlog {
            return Err(ObjectError::TryAgain);
        }
        pending.push_back(server_socket);
        *self.state.lock() = UnixSocketState::Stream(client_stream);
        if let Some(owner) = listener.owner.lock().as_ref().and_then(alloc::sync::Weak::upgrade) {
            wake_pollers(&owner, PollableEvent::CanBeRead);
        }
        wake_io();
        Ok(())
    }
}
