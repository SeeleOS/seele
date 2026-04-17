use alloc::{vec, vec::Vec};
use core::cmp::min;

use super::{AF_UNIX, SocketError, SocketResult, UnixSocketObject, UnixSocketState};

const SOCKADDR_UN_PATH_LEN: usize = 108;
const SA_FAMILY_LEN: usize = 2;

fn serialize_unix_addr(path: Option<&str>) -> Vec<u8> {
    let mut out = vec![0u8; SA_FAMILY_LEN];
    out[..SA_FAMILY_LEN].copy_from_slice(&(AF_UNIX as u16).to_ne_bytes());

    if let Some(path) = path {
        let path_bytes = path.as_bytes();
        let copy_len = if path_bytes.first() == Some(&0) {
            min(path_bytes.len(), SOCKADDR_UN_PATH_LEN)
        } else {
            min(path_bytes.len(), SOCKADDR_UN_PATH_LEN.saturating_sub(1))
        };
        out.resize(SA_FAMILY_LEN + SOCKADDR_UN_PATH_LEN, 0);
        out[SA_FAMILY_LEN..SA_FAMILY_LEN + copy_len].copy_from_slice(&path_bytes[..copy_len]);
    }

    out
}

impl UnixSocketObject {
    pub fn getsockname_bytes(&self) -> SocketResult<Vec<u8>> {
        match &*self.state.lock() {
            UnixSocketState::Unbound => Ok(serialize_unix_addr(None)),
            UnixSocketState::Bound { path } => Ok(serialize_unix_addr(Some(path))),
            UnixSocketState::Listener(listener) => Ok(serialize_unix_addr(Some(&listener.path))),
            UnixSocketState::Stream(stream) => {
                let local_name = stream.local_name.lock();
                Ok(serialize_unix_addr(local_name.as_deref()))
            }
            UnixSocketState::Closed => Err(SocketError::InvalidArguments),
        }
    }

    pub fn getpeername_bytes(&self) -> SocketResult<Vec<u8>> {
        match &*self.state.lock() {
            UnixSocketState::Stream(stream) => {
                let peer_name = stream.peer_name.lock();
                Ok(serialize_unix_addr(peer_name.as_deref()))
            }
            _ => Err(SocketError::InvalidArguments),
        }
    }

    pub fn shutdown(&self, how: u64) -> SocketResult<()> {
        let stream = match &*self.state.lock() {
            UnixSocketState::Stream(stream) => stream.clone(),
            _ => return Err(SocketError::InvalidArguments),
        };

        match how {
            0 => {
                *stream.read_shutdown.lock() = true;
            }
            1 => {
                if !*stream.write_shutdown.lock() {
                    *stream.write_shutdown.lock() = true;
                    stream.close_local();
                }
            }
            2 => {
                *stream.read_shutdown.lock() = true;
                if !*stream.write_shutdown.lock() {
                    *stream.write_shutdown.lock() = true;
                    stream.close_local();
                }
            }
            _ => return Err(SocketError::InvalidArguments),
        }

        Ok(())
    }
}
