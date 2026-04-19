use alloc::sync::Arc;
use spin::Mutex;

use crate::object::FileFlags;

use super::{
    AF_UNIX, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_STREAM, SocketError, SocketResult,
    UnixDatagramInner, UnixSocketKind, UnixSocketObject, UnixSocketState,
};

impl UnixSocketObject {
    pub fn new(kind: UnixSocketKind) -> Self {
        let state = match kind {
            UnixSocketKind::Stream => UnixSocketState::Unbound,
            UnixSocketKind::Datagram => {
                UnixSocketState::Datagram(Arc::new(UnixDatagramInner::new()))
            }
        };
        Self {
            kind,
            state: Mutex::new(state),
            flags: Mutex::new(FileFlags::empty()),
        }
    }

    pub fn create(domain: u64, kind: u64, protocol: u64) -> SocketResult<Arc<Self>> {
        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        if domain != AF_UNIX {
            return Err(SocketError::AddressFamilyNotSupported);
        }
        if protocol != 0 {
            return Err(SocketError::ProtocolNotSupported);
        }

        let kind = match socket_type {
            SOCK_STREAM => UnixSocketKind::Stream,
            SOCK_DGRAM => UnixSocketKind::Datagram,
            _ => return Err(SocketError::ProtocolNotSupported),
        };

        let socket = Arc::new(Self::new(kind));
        if let UnixSocketState::Datagram(datagram) = &*socket.state.lock() {
            *datagram.owner.lock() = Some(Arc::downgrade(&socket));
        }
        Ok(socket)
    }

    pub fn is_nonblocking(&self) -> bool {
        self.flags.lock().contains(FileFlags::NONBLOCK)
    }
}

impl Default for UnixSocketObject {
    fn default() -> Self {
        Self::new(UnixSocketKind::Stream)
    }
}
