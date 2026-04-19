use alloc::sync::Arc;
use spin::Mutex;

use crate::object::FileFlags;

use super::{
    AF_UNIX, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_SEQPACKET, SOCK_STREAM, SocketError,
    SocketResult, UnixDatagramInner, UnixSocketKind, UnixSocketObject, UnixSocketState,
    UnixStreamInner,
};

impl UnixSocketObject {
    pub fn pair(domain: u64, kind: u64, protocol: u64) -> SocketResult<(Arc<Self>, Arc<Self>)> {
        if domain != AF_UNIX {
            return Err(SocketError::AddressFamilyNotSupported);
        }

        let socket_type = kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        if protocol != 0 {
            return Err(SocketError::ProtocolNotSupported);
        }

        let (left, right) = match socket_type {
            SOCK_STREAM | SOCK_SEQPACKET => {
                let kind = if socket_type == SOCK_SEQPACKET {
                    UnixSocketKind::SeqPacket
                } else {
                    UnixSocketKind::Stream
                };
                let (left_stream, right_stream) = UnixStreamInner::pair();
                let left = Arc::new(Self {
                    kind,
                    state: Mutex::new(UnixSocketState::Stream(left_stream.clone())),
                    flags: Mutex::new(FileFlags::empty()),
                    pass_cred: Mutex::new(false),
                });
                let right = Arc::new(Self {
                    kind,
                    state: Mutex::new(UnixSocketState::Stream(right_stream.clone())),
                    flags: Mutex::new(FileFlags::empty()),
                    pass_cred: Mutex::new(false),
                });

                *left_stream.owner.lock() = Some(Arc::downgrade(&left));
                *right_stream.owner.lock() = Some(Arc::downgrade(&right));
                (left, right)
            }
            SOCK_DGRAM => {
                let left_inner = Arc::new(UnixDatagramInner::new());
                let right_inner = Arc::new(UnixDatagramInner::new());
                let left = Arc::new(Self {
                    kind: UnixSocketKind::Datagram,
                    state: Mutex::new(UnixSocketState::Datagram(left_inner.clone())),
                    flags: Mutex::new(FileFlags::empty()),
                    pass_cred: Mutex::new(false),
                });
                let right = Arc::new(Self {
                    kind: UnixSocketKind::Datagram,
                    state: Mutex::new(UnixSocketState::Datagram(right_inner.clone())),
                    flags: Mutex::new(FileFlags::empty()),
                    pass_cred: Mutex::new(false),
                });

                *left_inner.owner.lock() = Some(Arc::downgrade(&left));
                *right_inner.owner.lock() = Some(Arc::downgrade(&right));
                *left_inner.peer.lock() = Some(Arc::downgrade(&right));
                *right_inner.peer.lock() = Some(Arc::downgrade(&left));
                (left, right)
            }
            _ => return Err(SocketError::ProtocolNotSupported),
        };

        if (kind & SOCK_NONBLOCK) != 0 {
            *left.flags.lock() = FileFlags::NONBLOCK;
            *right.flags.lock() = FileFlags::NONBLOCK;
        }

        Ok((left, right))
    }
}
