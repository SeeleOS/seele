use super::{UNIX_SOCKET_REGISTRY, UnixSocketObject, UnixSocketState, wake_io};

impl Drop for UnixSocketObject {
    fn drop(&mut self) {
        match &*self.state.lock() {
            UnixSocketState::Bound { path } => {
                UNIX_SOCKET_REGISTRY.lock().remove(path);
            }
            UnixSocketState::Listener(listener) => {
                UNIX_SOCKET_REGISTRY.lock().remove(&listener.path);
                wake_io();
            }
            UnixSocketState::Stream(stream) => stream.close_local(),
            UnixSocketState::Unbound | UnixSocketState::Closed => {}
        }
    }
}
