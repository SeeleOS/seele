use crate::{memory::utils::Mut, object::bpf::BpfObject};
use alloc::sync::{Arc, Weak};

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
    pub state: Mut<UnixSocketState>,
    pub flags: Mut<FileFlags>,
    pub pass_cred: Mut<bool>,
    pub priority: Mut<i32>,
    pub attached_bpf: Mut<Option<Arc<BpfObject>>>,
    pub self_ref: Mut<Option<Weak<UnixSocketObject>>>,
    pub creator_cred: SocketPeerCred,
}
