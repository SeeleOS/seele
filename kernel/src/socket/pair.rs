use crate::memory::utils::Mut;
use alloc::sync::Arc;

use crate::object::FileFlags;

use super::{
    AF_UNIX, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_SEQPACKET, SOCK_STREAM, SocketError,
    SocketResult, UnixDatagramInner, UnixSocketKind, UnixSocketObject, UnixSocketState,
    UnixStreamInner, current_socket_peer_cred,
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
                let creator_cred = current_socket_peer_cred();
                let left = Arc::new(Self {
                    kind,
                    state: Mut::new(UnixSocketState::Stream(left_stream.clone())),
                    flags: Mut::new(FileFlags::empty()),
                    pass_cred: Mut::new(false),
                    priority: Mut::new(0),
                    creator_cred,
                });
                let right = Arc::new(Self {
                    kind,
                    state: Mut::new(UnixSocketState::Stream(right_stream.clone())),
                    flags: Mut::new(FileFlags::empty()),
                    pass_cred: Mut::new(false),
                    priority: Mut::new(0),
                    creator_cred,
                });

                *left_stream.owner.lock() = Some(Arc::downgrade(&left));
                *right_stream.owner.lock() = Some(Arc::downgrade(&right));
                *left_stream.peer_cred.lock() = right.creator_cred;
                *right_stream.peer_cred.lock() = left.creator_cred;
                (left, right)
            }
            SOCK_DGRAM => {
                let left_inner = Arc::new(UnixDatagramInner::new());
                let right_inner = Arc::new(UnixDatagramInner::new());
                let creator_cred = current_socket_peer_cred();
                let left = Arc::new(Self {
                    kind: UnixSocketKind::Datagram,
                    state: Mut::new(UnixSocketState::Datagram(left_inner.clone())),
                    flags: Mut::new(FileFlags::empty()),
                    pass_cred: Mut::new(false),
                    priority: Mut::new(0),
                    creator_cred,
                });
                let right = Arc::new(Self {
                    kind: UnixSocketKind::Datagram,
                    state: Mut::new(UnixSocketState::Datagram(right_inner.clone())),
                    flags: Mut::new(FileFlags::empty()),
                    pass_cred: Mut::new(false),
                    priority: Mut::new(0),
                    creator_cred,
                });

                *left_inner.owner.lock() = Some(Arc::downgrade(&left));
                *right_inner.owner.lock() = Some(Arc::downgrade(&right));
                *left_inner.peer.lock() = Some(Arc::downgrade(&right));
                *right_inner.peer.lock() = Some(Arc::downgrade(&left));
                *left_inner.peer_cred.lock() = right.creator_cred;
                *right_inner.peer_cred.lock() = left.creator_cred;
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
