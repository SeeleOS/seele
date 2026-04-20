use super::{
    UNIX_SOCKET_REGISTRY, UnixSocketObject, UnixSocketRegistryEntry, UnixSocketState, wake_io,
};
use crate::filesystem::{path::Path, vfs::VirtualFS};

impl Drop for UnixSocketObject {
    fn drop(&mut self) {
        match &*self.state.lock() {
            UnixSocketState::Bound { path } => {
                UNIX_SOCKET_REGISTRY.lock().remove(path);
                if path.as_bytes().first() != Some(&0) {
                    let _ = VirtualFS.lock().delete_file(Path::new(path));
                }
            }
            UnixSocketState::Listener(listener) => {
                UNIX_SOCKET_REGISTRY.lock().remove(&listener.path);
                if listener.path.as_bytes().first() != Some(&0) {
                    let _ = VirtualFS.lock().delete_file(Path::new(&listener.path));
                }
                wake_io();
            }
            UnixSocketState::Datagram(datagram) => {
                if let Some(path) = datagram.local_name.lock().clone() {
                    UNIX_SOCKET_REGISTRY.lock().remove(&path);
                    if path.as_bytes().first() != Some(&0) {
                        let _ = VirtualFS.lock().delete_file(Path::new(&path));
                    }
                }
                datagram.close_local();
            }
            UnixSocketState::Stream(stream) => {
                if let Some(path) = stream.local_name.lock().clone() {
                    let should_remove = matches!(
                        UNIX_SOCKET_REGISTRY.lock().get(&path),
                        Some(UnixSocketRegistryEntry::StreamReserved)
                    );
                    if should_remove {
                        UNIX_SOCKET_REGISTRY.lock().remove(&path);
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
