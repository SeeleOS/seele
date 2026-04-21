use super::{
    UNIX_SOCKET_REGISTRY, UnixSocketObject, UnixSocketRegistryEntry, UnixSocketState, wake_io,
};
use crate::filesystem::{path::Path, vfs::VirtualFS};

impl Drop for UnixSocketObject {
    fn drop(&mut self) {
        match &*self.state.lock() {
            UnixSocketState::Bound { path, registry_key } => {
                UNIX_SOCKET_REGISTRY.lock().remove(registry_key);
                if path.as_bytes().first() != Some(&0) {
                    let _ = VirtualFS.lock().delete_file(Path::new(path));
                }
            }
            UnixSocketState::Listener(listener) => {
                UNIX_SOCKET_REGISTRY.lock().remove(&listener.registry_key);
                if listener.path.as_bytes().first() != Some(&0) {
                    let _ = VirtualFS.lock().delete_file(Path::new(&listener.path));
                }
                wake_io();
            }
            UnixSocketState::Datagram(datagram) => {
                if let Some(path) = datagram.local_name.lock().clone() {
                    if let Some(registry_key) = datagram.local_key.lock().clone() {
                        UNIX_SOCKET_REGISTRY.lock().remove(&registry_key);
                    }
                    if path.as_bytes().first() != Some(&0) {
                        let _ = VirtualFS.lock().delete_file(Path::new(&path));
                    }
                }
                datagram.close_local();
            }
            UnixSocketState::Stream(stream) => {
                if let Some(path) = stream.local_name.lock().clone() {
                    let local_key = stream.local_key.lock().clone();
                    let should_remove = if let Some(registry_key) = local_key.as_ref() {
                        matches!(
                            UNIX_SOCKET_REGISTRY.lock().get(registry_key),
                            Some(UnixSocketRegistryEntry::StreamReserved)
                        )
                    } else {
                        false
                    };
                    if should_remove {
                        if let Some(registry_key) = local_key {
                            UNIX_SOCKET_REGISTRY.lock().remove(&registry_key);
                        }
                        if path.as_bytes().first() != Some(&0) {
                            let _ = VirtualFS.lock().delete_file(Path::new(&path));
                        }
                    }
                }
                stream.close_local();
            }
            UnixSocketState::Unbound | UnixSocketState::Closed => {}
        }
    }
}
