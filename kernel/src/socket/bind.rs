use alloc::{string::String, sync::Arc};

use super::{
    SocketError, SocketResult, UNIX_SOCKET_REGISTRY, UnixListenerInner, UnixSocketKind,
    UnixSocketObject, UnixSocketRegistryEntry, UnixSocketState,
};
use crate::filesystem::{errors::FSError, path::Path, vfs::VirtualFS};

impl UnixSocketObject {
    pub fn bind(self: &Arc<Self>, path: String) -> SocketResult<()> {
        let mut state = self.state.lock();
        let can_bind = match (&self.kind, &*state) {
            (kind, UnixSocketState::Unbound) if kind.is_stream_like() => true,
            (UnixSocketKind::Datagram, UnixSocketState::Datagram(datagram)) => {
                datagram.local_name.lock().is_none()
            }
            _ => false,
        };
        if !can_bind {
            return Err(SocketError::InvalidArguments);
        }

        let is_abstract = path.as_bytes().first() == Some(&0);
        let registry = UNIX_SOCKET_REGISTRY.lock();
        if registry.contains_key(&path) {
            return Err(SocketError::AddressInUse);
        }
        drop(registry);

        if !is_abstract {
            let mut vfs = VirtualFS.lock();
            match vfs.create_file(Path::new(&path)) {
                Ok(()) => {}
                Err(FSError::AlreadyExists) => {
                    // Pathname sockets can leave a stale inode behind after an
                    // unclean exit. Remove it and recreate the node if the
                    // in-kernel listener registry no longer owns the path.
                    vfs.delete_file(Path::new(&path))
                        .map_err(|_| SocketError::AddressInUse)?;
                    vfs.create_file(Path::new(&path))
                        .map_err(|_| SocketError::AddressInUse)?;
                }
                Err(_) => return Err(SocketError::InvalidArguments),
            }
        }

        let mut registry = UNIX_SOCKET_REGISTRY.lock();
        if registry.contains_key(&path) {
            if !is_abstract {
                let _ = VirtualFS.lock().delete_file(Path::new(&path));
            }
            return Err(SocketError::AddressInUse);
        }
        match self.kind {
            UnixSocketKind::Stream | UnixSocketKind::SeqPacket => {
                registry.insert(path.clone(), UnixSocketRegistryEntry::StreamReserved);
                *state = UnixSocketState::Bound { path };
            }
            UnixSocketKind::Datagram => {
                registry.insert(
                    path.clone(),
                    UnixSocketRegistryEntry::Datagram(Arc::downgrade(self)),
                );
                let datagram = match &*state {
                    UnixSocketState::Datagram(datagram) => datagram.clone(),
                    _ => return Err(SocketError::InvalidArguments),
                };
                *datagram.local_name.lock() = Some(path);
            }
        }
        Ok(())
    }

    pub fn listen(self: &Arc<Self>, backlog: usize) -> SocketResult<()> {
        if !self.kind.is_stream_like() {
            return Err(SocketError::InvalidArguments);
        }
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
        *slot = UnixSocketRegistryEntry::Listener(listener.clone());
        *self.state.lock() = UnixSocketState::Listener(listener);
        Ok(())
    }
}
