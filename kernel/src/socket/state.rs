use crate::memory::utils::Mut;
use alloc::{
    collections::VecDeque,
    string::String,
    sync::{Arc, Weak},
};

use super::{
    UnixDatagramInner, UnixSocketObject, UnixStreamInner, registry::UnixSocketRegistryKey,
};

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
    pub pending: Mut<VecDeque<Arc<UnixSocketObject>>>,
    pub owner: Mut<Option<Weak<UnixSocketObject>>>,
}

impl UnixListenerInner {
    pub fn new(path: String, registry_key: UnixSocketRegistryKey, backlog: usize) -> Self {
        Self {
            path,
            registry_key,
            backlog,
            pending: Mut::new(VecDeque::new()),
            owner: Mut::new(None),
        }
    }
}
