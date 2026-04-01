use alloc::sync::Arc;
use seele_sys::{abi::object::ObjectFlags, abi::socket};
use spin::Mutex;

use super::{SocketError, SocketResult, UnixSocketObject, UnixSocketState};

impl UnixSocketObject {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(UnixSocketState::Unbound),
            flags: Mutex::new(ObjectFlags::empty()),
        }
    }

    pub fn create(domain: u64, kind: u64, protocol: u64) -> SocketResult<Arc<Self>> {
        if domain != socket::AF_UNIX {
            return Err(SocketError::AddressFamilyNotSupported);
        }
        if kind != socket::SOCK_STREAM || protocol != 0 {
            return Err(SocketError::ProtocolNotSupported);
        }

        Ok(Arc::new(Self::new()))
    }

    pub fn is_nonblocking(&self) -> bool {
        self.flags.lock().contains(ObjectFlags::NONBLOCK)
    }
}

impl Default for UnixSocketObject {
    fn default() -> Self {
        Self::new()
    }
}
