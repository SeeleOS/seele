use alloc::sync::Arc;
use spin::Mutex;

use crate::object::FileFlags;

use super::{
    AF_UNIX, SOCK_CLOEXEC, SOCK_NONBLOCK, SOCK_STREAM, SocketError, SocketResult, UnixSocketObject,
    UnixSocketState, UnixStreamInner,
};

impl UnixSocketObject {
    pub fn pair(domain: u64, kind: u64, protocol: u64) -> SocketResult<(Arc<Self>, Arc<Self>)> {
        if domain != AF_UNIX {
            return Err(SocketError::AddressFamilyNotSupported);
        }

        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        if socket_type != SOCK_STREAM || protocol != 0 {
            return Err(SocketError::ProtocolNotSupported);
        }

        let (left_stream, right_stream) = UnixStreamInner::pair();
        let left = Arc::new(Self {
            state: Mutex::new(UnixSocketState::Stream(left_stream.clone())),
            flags: Mutex::new(FileFlags::empty()),
        });
        let right = Arc::new(Self {
            state: Mutex::new(UnixSocketState::Stream(right_stream.clone())),
            flags: Mutex::new(FileFlags::empty()),
        });

        *left_stream.owner.lock() = Some(Arc::downgrade(&left));
        *right_stream.owner.lock() = Some(Arc::downgrade(&right));

        if (kind & SOCK_NONBLOCK) != 0 {
            *left.flags.lock() = FileFlags::NONBLOCK;
            *right.flags.lock() = FileFlags::NONBLOCK;
        }

        Ok((left, right))
    }
}
