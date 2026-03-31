use alloc::{collections::VecDeque, string::String, sync::{Arc, Weak}};
use spin::Mutex;

use super::{UnixSocketObject, UnixStreamInner};

#[derive(Debug)]
pub enum UnixSocketState {
    Unbound,
    Bound { path: String },
    Listener(Arc<UnixListenerInner>),
    Stream(Arc<UnixStreamInner>),
    Closed,
}

#[derive(Debug)]
pub struct UnixListenerInner {
    pub path: String,
    pub backlog: usize,
    pub pending: Mutex<VecDeque<Arc<UnixSocketObject>>>,
    pub owner: Mutex<Option<Weak<UnixSocketObject>>>,
}

impl UnixListenerInner {
    pub fn new(path: String, backlog: usize) -> Self {
        Self {
            path,
            backlog,
            pending: Mutex::new(VecDeque::new()),
            owner: Mutex::new(None),
        }
    }
}
