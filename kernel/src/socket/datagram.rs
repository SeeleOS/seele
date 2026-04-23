use alloc::{collections::VecDeque, string::String, sync::Weak, vec::Vec};
use spin::Mutex;

use super::{
    SocketPeerCred, UnixSocketObject, registry::UnixSocketRegistryKey, wake_io, wake_pollers,
};
use crate::polling::event::PollableEvent;

pub const DATAGRAM_RECV_CAPACITY: usize = 64 * 1024;

#[derive(Debug)]
pub struct UnixDatagramMessage {
    pub data: Vec<u8>,
    pub sender_name: Option<String>,
    pub sender_cred: SocketPeerCred,
}

#[derive(Debug)]
pub struct UnixDatagramInner {
    pub recv_queue: Mutex<VecDeque<UnixDatagramMessage>>,
    pub owner: Mutex<Option<Weak<UnixSocketObject>>>,
    pub peer: Mutex<Option<Weak<UnixSocketObject>>>,
    pub local_name: Mutex<Option<String>>,
    pub local_key: Mutex<Option<UnixSocketRegistryKey>>,
    pub peer_name: Mutex<Option<String>>,
    pub peer_key: Mutex<Option<UnixSocketRegistryKey>>,
    pub peer_cred: Mutex<SocketPeerCred>,
    pub read_shutdown: Mutex<bool>,
    pub write_shutdown: Mutex<bool>,
}

impl UnixDatagramInner {
    pub fn new() -> Self {
        Self {
            recv_queue: Mutex::new(VecDeque::new()),
            owner: Mutex::new(None),
            peer: Mutex::new(None),
            local_name: Mutex::new(None),
            local_key: Mutex::new(None),
            peer_name: Mutex::new(None),
            peer_key: Mutex::new(None),
            peer_cred: Mutex::new(SocketPeerCred::default()),
            read_shutdown: Mutex::new(false),
            write_shutdown: Mutex::new(false),
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
