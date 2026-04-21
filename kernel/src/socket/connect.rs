use alloc::{
    string::String,
    sync::{Arc, Weak},
};
use spin::Mutex;

use super::{
    SocketError, SocketPeerCred, SocketResult, UNIX_SOCKET_REGISTRY, UnixSocketKind,
    UnixSocketObject, UnixSocketRegistryEntry, UnixSocketRegistryKey, UnixSocketState,
    UnixStreamInner, wake_io, wake_pollers,
};
use crate::object::FileFlags;
use crate::polling::event::PollableEvent;
use crate::process::manager::get_current_process;

impl UnixSocketObject {
    pub fn connect(self: &Arc<Self>, path: String) -> SocketResult<()> {
        match self.kind {
            UnixSocketKind::Stream | UnixSocketKind::SeqPacket => {
                let registry_key = UnixSocketRegistryKey::from_socket_path(&path)
                    .ok_or(SocketError::ConnectionRefused)?;
                let listener = {
                    let registry = UNIX_SOCKET_REGISTRY.lock();
                    match registry.get(&registry_key) {
                        Some(UnixSocketRegistryEntry::Listener(listener)) => listener.clone(),
                        _ => return Err(SocketError::ConnectionRefused),
                    }
                };

                let (local_name, local_key) = match &*self.state.lock() {
                    UnixSocketState::Unbound => (None, None),
                    UnixSocketState::Bound { path, registry_key } => {
                        (Some(path.clone()), Some(registry_key.clone()))
                    }
                    UnixSocketState::Stream(_) => return Err(SocketError::IsConnected),
                    _ => return Err(SocketError::InvalidArguments),
                };

                let (client_stream, server_stream) = UnixStreamInner::pair();
                let peer_pid = get_current_process().lock().pid.0;
                *client_stream.owner.lock() = Some(Arc::downgrade(self));
                let kind = self.kind;
                let server_socket = Arc::new(Self {
                    kind,
                    state: Mutex::new(UnixSocketState::Stream(server_stream.clone())),
                    flags: Mutex::new(FileFlags::empty()),
                    pass_cred: Mutex::new(false),
                });
                *server_stream.owner.lock() = Some(Arc::downgrade(&server_socket));
                *server_stream.peer_cred.lock() = SocketPeerCred {
                    pid: peer_pid,
                    uid: 0,
                    gid: 0,
                };
                *client_stream.local_name.lock() = local_name.clone();
                *client_stream.local_key.lock() = local_key;
                *client_stream.peer_name.lock() = Some(listener.path.clone());
                *server_stream.peer_name.lock() = local_name;
                *server_stream.local_name.lock() = Some(listener.path.clone());
                *server_stream.local_key.lock() = Some(listener.registry_key.clone());

                let mut pending = listener.pending.lock();
                if pending.len() >= listener.backlog {
                    return Err(SocketError::TryAgain);
                }
                pending.push_back(server_socket);
                *self.state.lock() = UnixSocketState::Stream(client_stream);
                if let Some(owner) = listener.owner.lock().as_ref().and_then(Weak::upgrade) {
                    wake_pollers(&owner, PollableEvent::CanBeRead);
                }
                wake_io();
                Ok(())
            }
            UnixSocketKind::Datagram => {
                let registry_key = UnixSocketRegistryKey::from_socket_path(&path)
                    .ok_or(SocketError::ConnectionRefused)?;
                let endpoint = {
                    let registry = UNIX_SOCKET_REGISTRY.lock();
                    match registry.get(&registry_key) {
                        Some(UnixSocketRegistryEntry::Datagram(endpoint)) => endpoint
                            .upgrade()
                            .ok_or(SocketError::ConnectionRefused)?,
                        _ => return Err(SocketError::ConnectionRefused),
                    }
                };

                let datagram = match &*self.state.lock() {
                    UnixSocketState::Datagram(datagram) => datagram.clone(),
                    _ => return Err(SocketError::InvalidArguments),
                };
                let peer_name = match &*endpoint.state.lock() {
                    UnixSocketState::Datagram(peer_datagram) => {
                        peer_datagram.local_name.lock().clone()
                    }
                    _ => None,
                };
                *datagram.peer.lock() = Some(Arc::downgrade(&endpoint));
                *datagram.peer_key.lock() = Some(registry_key);
                *datagram.peer_name.lock() = peer_name.or(Some(path));
                Ok(())
            }
        }
    }
}
