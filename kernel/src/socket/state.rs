use alloc::{
    collections::VecDeque,
    string::String,
    sync::{Arc, Weak},
};
use spin::Mutex;

use super::{registry::UnixSocketRegistryKey, UnixDatagramInner, UnixSocketObject, UnixStreamInner};

#[derive(Debug)]
pub enum UnixSocketState {
    Unbound,
    Bound {
        path: String,
        registry_key: UnixSocketRegistryKey,
    },
    Listener(Arc<UnixListenerInner>),
    Datagram(Arc<UnixDatagramInner>),
    Stream(Arc<UnixStreamInner>),
    Closed,
}

#[derive(Debug)]
pub struct UnixListenerInner {
    pub path: String,
    pub registry_key: UnixSocketRegistryKey,
    pub backlog: usize,
    pub pending: Mutex<VecDeque<Arc<UnixSocketObject>>>,
    pub owner: Mutex<Option<Weak<UnixSocketObject>>>,
}

impl UnixListenerInner {
    pub fn new(path: String, registry_key: UnixSocketRegistryKey, backlog: usize) -> Self {
        Self {
            path,
            registry_key,
            backlog,
            pending: Mutex::new(VecDeque::new()),
            owner: Mutex::new(None),
        }
    }
}
