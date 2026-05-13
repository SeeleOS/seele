use crate::memory::utils::Mut;
use alloc::{collections::VecDeque, string::String, sync::Weak, vec::Vec};

use super::{
    SocketPeerCred, UnixSocketObject, registry::UnixSocketRegistryKey, wake_io, wake_pollers,
};
use crate::{object::misc::ObjectRef, polling::event::PollableEvent};

pub const DATAGRAM_RECV_CAPACITY: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct UnixDatagramMessage {
    pub data: Vec<u8>,
    pub sender_name: Option<String>,
    pub sender_cred: SocketPeerCred,
    pub rights: Vec<ObjectRef>,
}

#[derive(Debug)]
pub struct UnixDatagramInner {
    pub recv_queue: Mut<VecDeque<UnixDatagramMessage>>,
    pub owner: Mut<Option<Weak<UnixSocketObject>>>,
    pub peer: Mut<Option<Weak<UnixSocketObject>>>,
    pub local_name: Mut<Option<String>>,
    pub local_key: Mut<Option<UnixSocketRegistryKey>>,
    pub peer_name: Mut<Option<String>>,
    pub peer_key: Mut<Option<UnixSocketRegistryKey>>,
    pub peer_cred: Mut<SocketPeerCred>,
    pub peer_rights: Mut<Vec<ObjectRef>>,
    pub read_shutdown: Mut<bool>,
    pub write_shutdown: Mut<bool>,
}

impl UnixDatagramInner {
    pub fn new() -> Self {
        Self {
            recv_queue: Mut::new(VecDeque::new()),
            owner: Mut::new(None),
            peer: Mut::new(None),
            local_name: Mut::new(None),
            local_key: Mut::new(None),
            peer_name: Mut::new(None),
            peer_key: Mut::new(None),
            peer_cred: Mut::new(SocketPeerCred::default()),
            peer_rights: Mut::new(Vec::new()),
            read_shutdown: Mut::new(false),
            write_shutdown: Mut::new(false),
        }
    }

    pub fn close_local(&self) {
        if let Some(owner) = self.owner.lock().as_ref().and_then(Weak::upgrade) {
            wake_pollers(&owner, PollableEvent::Closed);
        }
        wake_io();
    }
}

impl Default for UnixDatagramInner {
    fn default() -> Self {
        Self::new()
    }
}
