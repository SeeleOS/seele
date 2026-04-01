use alloc::{string::String, sync::Arc};

use super::{
    SocketError, SocketResult, UNIX_SOCKET_REGISTRY, UnixListenerInner, UnixSocketObject,
    UnixSocketState,
};

impl UnixSocketObject {
    pub fn bind(self: &Arc<Self>, path: String) -> SocketResult<()> {
        let mut state = self.state.lock();
        if !matches!(*state, UnixSocketState::Unbound) {
            return Err(SocketError::InvalidArguments);
        }

        let mut registry = UNIX_SOCKET_REGISTRY.lock();
        if registry.contains_key(&path) {
            return Err(SocketError::AddressInUse);
        }

        registry.insert(path.clone(), None);
        *state = UnixSocketState::Bound { path };
        Ok(())
    }

    pub fn listen(self: &Arc<Self>, backlog: usize) -> SocketResult<()> {
        let path = match &*self.state.lock() {
            UnixSocketState::Bound { path } => path.clone(),
            _ => return Err(SocketError::InvalidArguments),
        };

        let listener = Arc::new(UnixListenerInner::new(path.clone(), backlog.max(1)));
        *listener.owner.lock() = Some(Arc::downgrade(self));

        let mut registry = UNIX_SOCKET_REGISTRY.lock();
        let slot = registry
            .get_mut(&path)
            .ok_or(SocketError::InvalidArguments)?;
        *slot = Some(listener.clone());
        *self.state.lock() = UnixSocketState::Listener(listener);
        Ok(())
    }
}
