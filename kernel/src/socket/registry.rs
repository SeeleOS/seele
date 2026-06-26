use crate::memory::utils::Mut;
use alloc::{
    collections::BTreeMap,
    string::String,
    sync::{Arc, Weak},
};
use lazy_static::lazy_static;

use crate::filesystem::{path::Path, vfs_operations::open_path};
use crate::object::traits::Statable;

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

        Self::from_resolved_path(Path::new(path))
    }

    pub fn from_resolved_path(path: Path) -> Option<Self> {
        let opened = open_path(path).ok()?;
        let stat = opened.stat();
        Some(Self::Path {
            mount_device_id: stat.st_dev,
            inode: stat.st_ino,
        })
    }
}

pub enum UnixSocketRegistryEntry {
    StreamReserved,
    Listener(Arc<UnixListenerInner>),
    Datagram(Weak<UnixSocketObject>),
}

lazy_static! {
    pub static ref UNIX_SOCKET_REGISTRY: Mut<BTreeMap<UnixSocketRegistryKey, UnixSocketRegistryEntry>> =
        Mut::new(BTreeMap::new());
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use super::UnixSocketRegistryKey;

    crate::test!(
        unix_socket_registry_abstract_keys,
        "unix socket registry keeps abstract names verbatim",
        unix_socket_registry_keeps_abstract_names_verbatim
    );

    fn unix_socket_registry_keeps_abstract_names_verbatim() {
        let key = UnixSocketRegistryKey::from_socket_path("\0wayland-0").unwrap();
        assert_eq!(
            key,
            UnixSocketRegistryKey::Abstract(String::from("\0wayland-0"))
        );
        assert!(UnixSocketRegistryKey::from_socket_path("/definitely/missing").is_none());
    }
}
