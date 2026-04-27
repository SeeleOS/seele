use alloc::{
    collections::VecDeque,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::Mutex;

use super::{UnixSocketObject, registry::UnixSocketRegistryKey, wake_io, wake_pollers};
use crate::{object::misc::ObjectRef, polling::event::PollableEvent};

pub const STREAM_RECV_CAPACITY: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct SocketPeerCred {
    pub pid: u64,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Clone, Debug)]
pub struct PendingRights {
    pub byte_offset: usize,
    pub rights: Vec<ObjectRef>,
}

#[derive(Debug)]
pub struct UnixStreamInner {
    pub recv_buf: Mutex<VecDeque<u8>>,
    pub pending_rights: Mutex<VecDeque<PendingRights>>,
    pub peer: Mutex<Option<Weak<UnixStreamInner>>>,
    pub owner: Mutex<Option<Weak<UnixSocketObject>>>,
    pub peer_cred: Mutex<SocketPeerCred>,
    pub write_closed: Mutex<bool>,
    pub read_shutdown: Mutex<bool>,
    pub write_shutdown: Mutex<bool>,
    pub local_name: Mutex<Option<String>>,
    pub local_key: Mutex<Option<UnixSocketRegistryKey>>,
    pub peer_name: Mutex<Option<String>>,
}

impl UnixStreamInner {
    pub fn new() -> Self {
        Self {
            recv_buf: Mutex::new(VecDeque::new()),
            pending_rights: Mutex::new(VecDeque::new()),
            peer: Mutex::new(None),
            owner: Mutex::new(None),
            peer_cred: Mutex::new(SocketPeerCred::default()),
            write_closed: Mutex::new(false),
            read_shutdown: Mutex::new(false),
            write_shutdown: Mutex::new(false),
            local_name: Mutex::new(None),
            local_key: Mutex::new(None),
            peer_name: Mutex::new(None),
        }
    }

    pub fn pair() -> (Arc<Self>, Arc<Self>) {
        let left = Arc::new(Self::new());
        let right = Arc::new(Self::new());

        *left.peer.lock() = Some(Arc::downgrade(&right));
        *right.peer.lock() = Some(Arc::downgrade(&left));

        (left, right)
    }

    pub fn close_local(&self) {
        if let Some(peer) = self.peer.lock().as_ref().and_then(Weak::upgrade) {
            *peer.write_closed.lock() = true;
            if let Some(owner) = peer.owner.lock().as_ref().and_then(Weak::upgrade) {
                wake_pollers(&owner, PollableEvent::CanBeRead);
                wake_pollers(&owner, PollableEvent::Closed);
                wake_pollers(&owner, PollableEvent::CanBeWritten);
            }
        }
        wake_io();
    }

    pub fn take_ready_rights(&self, bytes_read: usize) -> Vec<Vec<ObjectRef>> {
        if bytes_read == 0 {
            return Vec::new();
        }

        let mut pending = self.pending_rights.lock();
        let mut ready = Vec::new();

        while pending
            .front()
            .is_some_and(|entry| entry.byte_offset < bytes_read)
        {
            let entry = pending
                .pop_front()
                .expect("front element must exist while draining ready rights");
            ready.push(entry.rights);
        }

        for entry in pending.iter_mut() {
            entry.byte_offset = entry.byte_offset.saturating_sub(bytes_read);
        }

        ready
    }

    pub fn peek_ready_rights(&self, bytes_read: usize) -> Vec<Vec<ObjectRef>> {
        if bytes_read == 0 {
            return Vec::new();
        }

        self.pending_rights
            .lock()
            .iter()
            .take_while(|entry| entry.byte_offset < bytes_read)
            .map(|entry| entry.rights.clone())
            .collect()
    }
}

impl Default for UnixStreamInner {
    fn default() -> Self {
        Self::new()
    }
}
