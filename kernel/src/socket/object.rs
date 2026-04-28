use spin::Mutex;

use crate::object::FileFlags;

use super::{SocketPeerCred, UnixSocketState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnixSocketKind {
    Stream,
    Datagram,
    SeqPacket,
}

impl UnixSocketKind {
    pub fn is_stream_like(self) -> bool {
        matches!(self, Self::Stream | Self::SeqPacket)
    }
}

#[derive(Debug)]
pub struct UnixSocketObject {
    pub kind: UnixSocketKind,
    pub state: Mutex<UnixSocketState>,
    pub flags: Mutex<FileFlags>,
    pub pass_cred: Mutex<bool>,
    pub creator_cred: SocketPeerCred,
}
