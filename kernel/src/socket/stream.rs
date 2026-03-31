use alloc::{
    collections::VecDeque,
    sync::{Arc, Weak},
};
use spin::Mutex;

use super::{UnixSocketObject, wake_io, wake_pollers};
use crate::polling::event::PollableEvent;

#[derive(Debug)]
pub struct UnixStreamInner {
    pub recv_buf: Mutex<VecDeque<u8>>,
    pub peer: Mutex<Option<Weak<UnixStreamInner>>>,
    pub owner: Mutex<Option<Weak<UnixSocketObject>>>,
    pub write_closed: Mutex<bool>,
}

impl UnixStreamInner {
    pub fn new() -> Self {
        Self {
            recv_buf: Mutex::new(VecDeque::new()),
            peer: Mutex::new(None),
            owner: Mutex::new(None),
            write_closed: Mutex::new(false),
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
            }
        }
        wake_io();
    }
}
