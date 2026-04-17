use alloc::sync::Arc;
use spin::Mutex;

use crate::object::FileFlags;

use super::{
    AF_UNIX, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_STREAM, SocketError, SocketResult, UnixSocketObject,
    UnixSocketState,
};

impl UnixSocketObject {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(UnixSocketState::Unbound),
            flags: Mutex::new(FileFlags::empty()),
        }
    }

    pub fn create(domain: u64, kind: u64, protocol: u64) -> SocketResult<Arc<Self>> {
        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        if domain != AF_UNIX {
            return Err(SocketError::AddressFamilyNotSupported);
        }
        if socket_type != SOCK_STREAM || protocol != 0 {
            return Err(SocketError::ProtocolNotSupported);
        }

        Ok(Arc::new(Self::new()))
    }

    pub fn is_nonblocking(&self) -> bool {
        self.flags.lock().contains(FileFlags::NONBLOCK)
    }
}

impl Default for UnixSocketObject {
    fn default() -> Self {
        Self::new()
    }
}
