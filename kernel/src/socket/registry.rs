use alloc::{
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::filesystem::{object::mount_device_id_for_path, path::Path, vfs::VirtualFS};

use super::{UnixListenerInner, UnixSocketObject};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnixSocketRegistryKey {
    Abstract(String),
    Path { mount_device_id: u64, inode: u64 },
}

impl UnixSocketRegistryKey {
    pub fn from_socket_path(path: &str) -> Option<Self> {
        if path.as_bytes().first() == Some(&0) {
            return Some(Self::Abstract(String::from(path)));
        }

        let opened = VirtualFS.lock().open(Path::new(path)).ok()?;
        let info = opened.info().ok()?;
        Some(Self::Path {
            mount_device_id: mount_device_id_for_path(&opened.path()),
            inode: info.inode,
        })
    }
}

pub enum UnixSocketRegistryEntry {
    StreamReserved,
    Listener(Arc<UnixListenerInner>),
    Datagram(Weak<UnixSocketObject>),
}

lazy_static! {
    pub static ref UNIX_SOCKET_REGISTRY: Mutex<BTreeMap<UnixSocketRegistryKey, UnixSocketRegistryEntry>> =
        Mutex::new(BTreeMap::new());
}
