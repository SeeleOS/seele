use alloc::{string::String, sync::Arc};

use super::{UNIX_SOCKET_REGISTRY, UnixListenerInner, UnixSocketObject, UnixSocketState};
use crate::object::{error::ObjectError, misc::ObjectResult};

impl UnixSocketObject {
    pub fn bind(self: &Arc<Self>, path: String) -> ObjectResult<()> {
        let mut state = self.state.lock();
        if !matches!(*state, UnixSocketState::Unbound) {
            return Err(ObjectError::InvalidArguments);
        }

        let mut registry = UNIX_SOCKET_REGISTRY.lock();
        if registry.contains_key(&path) {
            return Err(ObjectError::AddressInUse);
        }

        registry.insert(path.clone(), None);
        *state = UnixSocketState::Bound { path };
        Ok(())
    }

    pub fn listen(self: &Arc<Self>, backlog: usize) -> ObjectResult<()> {
        let path = match &*self.state.lock() {
            UnixSocketState::Bound { path } => path.clone(),
            _ => return Err(ObjectError::InvalidArguments),
        };

        let listener = Arc::new(UnixListenerInner::new(path.clone(), backlog.max(1)));
        *listener.owner.lock() = Some(Arc::downgrade(self));

        let mut registry = UNIX_SOCKET_REGISTRY.lock();
        let slot = registry.get_mut(&path).ok_or(ObjectError::InvalidArguments)?;
        *slot = Some(listener.clone());
        *self.state.lock() = UnixSocketState::Listener(listener);
        Ok(())
    }
}
