use alloc::{string::String, sync::Arc};
use spin::Mutex;

use super::{
    SocketError, SocketPeerCred, SocketResult, UNIX_SOCKET_REGISTRY, UnixSocketKind,
    UnixSocketObject, UnixSocketRegistryEntry, UnixSocketState, UnixStreamInner, wake_io,
    wake_pollers,
};
use crate::object::FileFlags;
use crate::polling::event::PollableEvent;
use crate::process::manager::get_current_process;

impl UnixSocketObject {
    pub fn connect(self: &Arc<Self>, path: String) -> SocketResult<()> {
        match self.kind {
            UnixSocketKind::Stream => {
                let listener = {
                    let registry = UNIX_SOCKET_REGISTRY.lock();
                    match registry.get(&path) {
                        Some(UnixSocketRegistryEntry::Listener(listener)) => listener.clone(),
                        _ => return Err(SocketError::ConnectionRefused),
                    }
                };

                match &*self.state.lock() {
                    UnixSocketState::Unbound => {}
                    UnixSocketState::Stream(_) => return Err(SocketError::IsConnected),
                    _ => return Err(SocketError::InvalidArguments),
                }

                let (client_stream, server_stream) = UnixStreamInner::pair();
                let peer_pid = get_current_process().lock().pid.0;
                *client_stream.owner.lock() = Some(Arc::downgrade(self));
                let server_socket = Arc::new(Self {
                    kind: UnixSocketKind::Stream,
                    state: Mutex::new(UnixSocketState::Stream(server_stream.clone())),
                    flags: Mutex::new(FileFlags::empty()),
                });
                *server_stream.owner.lock() = Some(Arc::downgrade(&server_socket));
                *server_stream.peer_cred.lock() = SocketPeerCred {
                    pid: peer_pid,
                    uid: 0,
                    gid: 0,
                };
                *client_stream.peer_name.lock() = Some(path.clone());
                *server_stream.local_name.lock() = Some(path.clone());

                let mut pending = listener.pending.lock();
                if pending.len() >= listener.backlog {
                    return Err(SocketError::TryAgain);
                }
                pending.push_back(server_socket);
                *self.state.lock() = UnixSocketState::Stream(client_stream);
                if let Some(owner) = listener
                    .owner
                    .lock()
                    .as_ref()
                    .and_then(alloc::sync::Weak::upgrade)
                {
                    wake_pollers(&owner, PollableEvent::CanBeRead);
                }
                wake_io();
                Ok(())
            }
            UnixSocketKind::Datagram => {
                let exists = {
                    let registry = UNIX_SOCKET_REGISTRY.lock();
                    matches!(
                        registry.get(&path),
                        Some(UnixSocketRegistryEntry::Datagram(endpoint))
                            if endpoint.upgrade().is_some()
                    )
                };
                if !exists {
                    return Err(SocketError::ConnectionRefused);
                }

                let datagram = match &*self.state.lock() {
                    UnixSocketState::Datagram(datagram) => datagram.clone(),
                    _ => return Err(SocketError::InvalidArguments),
                };
                *datagram.peer_name.lock() = Some(path);
                Ok(())
            }
        }
    }
}
