use alloc::{
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
};
use lazy_static::lazy_static;
use spin::Mutex;

use super::{UnixListenerInner, UnixSocketObject};

pub enum UnixSocketRegistryEntry {
    StreamReserved,
    Listener(Arc<UnixListenerInner>),
    Datagram(Weak<UnixSocketObject>),
}

lazy_static! {
    pub static ref UNIX_SOCKET_REGISTRY: Mutex<BTreeMap<String, UnixSocketRegistryEntry>> =
        Mutex::new(BTreeMap::new());
}
