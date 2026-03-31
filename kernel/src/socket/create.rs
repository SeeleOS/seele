use alloc::sync::Arc;
use seele_sys::{abi::object::ObjectFlags, abi::socket};
use spin::Mutex;

use super::{UnixSocketObject, UnixSocketState};
use crate::object::{error::ObjectError, misc::ObjectResult};

impl UnixSocketObject {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(UnixSocketState::Unbound),
            flags: Mutex::new(ObjectFlags::empty()),
        }
    }

    pub fn create(domain: u64, kind: u64, protocol: u64) -> ObjectResult<Arc<Self>> {
        if domain != socket::AF_UNIX {
            return Err(ObjectError::AddressFamilyNotSupported);
        }
        if kind != socket::SOCK_STREAM || protocol != 0 {
            return Err(ObjectError::ProtocolNotSupported);
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
