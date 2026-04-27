use alloc::sync::Weak;

use super::{
    DATAGRAM_RECV_CAPACITY, STREAM_RECV_CAPACITY, SocketError, SocketPeerCred, SocketResult,
    UNIX_SOCKET_REGISTRY, UnixDatagramMessage, UnixSocketKind, UnixSocketObject,
    UnixSocketRegistryEntry, UnixSocketRegistryKey, UnixSocketState, wake_io, wake_pollers,
};
use crate::{
    object::{error::ObjectError, traits::Writable},
    polling::event::PollableEvent,
    process::manager::get_current_process,
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

impl Writable for UnixSocketObject {
    fn write(&self, buffer: &[u8]) -> Result<usize, ObjectError> {
        self.write_socket(buffer).map_err(Into::into)
    }
}

impl UnixSocketObject {
    fn write_datagram_socket(
        &self,
        buffer: &[u8],
        target_path: Option<&str>,
        force_nonblocking: bool,
    ) -> SocketResult<usize> {
        let nonblocking = force_nonblocking || self.is_nonblocking();
        let datagram = match &*self.state.lock() {
            UnixSocketState::Datagram(datagram) => datagram.clone(),
            _ => return Err(SocketError::InvalidArguments),
        };

        if *datagram.write_shutdown.lock() {
            return Err(SocketError::BrokenPipe);
        }

        let peer = if let Some(target_path) = target_path {
            let target_key = UnixSocketRegistryKey::from_socket_path(target_path)
                .ok_or(SocketError::ConnectionRefused)?;
            let endpoint = {
                let registry = UNIX_SOCKET_REGISTRY.lock();
                match registry.get(&target_key) {
                    Some(UnixSocketRegistryEntry::Datagram(endpoint)) => endpoint.upgrade(),
                    _ => None,
                }
            };
            endpoint.ok_or(SocketError::ConnectionRefused)?
        } else if let Some(peer) = datagram.peer.lock().as_ref().and_then(Weak::upgrade) {
            peer
        } else if let Some(peer_key) = datagram.peer_key.lock().clone() {
            let registry = UNIX_SOCKET_REGISTRY.lock();
            match registry.get(&peer_key) {
                Some(UnixSocketRegistryEntry::Datagram(endpoint)) => {
                    endpoint.upgrade().ok_or(SocketError::ConnectionRefused)?
                }
                _ => return Err(SocketError::ConnectionRefused),
            }
        } else {
            let peer_name = datagram
                .peer_name
                .lock()
                .clone()
                .ok_or(SocketError::ConnectionRefused)?;
            let peer_key = UnixSocketRegistryKey::from_socket_path(&peer_name)
                .ok_or(SocketError::ConnectionRefused)?;
            let endpoint = {
                let registry = UNIX_SOCKET_REGISTRY.lock();
                match registry.get(&peer_key) {
                    Some(UnixSocketRegistryEntry::Datagram(endpoint)) => endpoint.upgrade(),
                    _ => None,
                }
            };
            endpoint.ok_or(SocketError::ConnectionRefused)?
        };
        let peer_datagram = match &*peer.state.lock() {
            UnixSocketState::Datagram(datagram) => datagram.clone(),
            _ => return Err(SocketError::ConnectionRefused),
        };

        if *peer_datagram.read_shutdown.lock() {
            return Err(SocketError::BrokenPipe);
        }

        let mut recv_queue = peer_datagram.recv_queue.lock();
        if recv_queue.len() >= DATAGRAM_RECV_CAPACITY {
            drop(recv_queue);
            if nonblocking {
                return Err(SocketError::TryAgain);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });
            if peer_datagram.recv_queue.lock().len() < DATAGRAM_RECV_CAPACITY
                || *peer_datagram.read_shutdown.lock()
            {
                cancel_block(&current);
            } else {
                finish_block_current();
            }
            return Ok(0);
        }

        recv_queue.push_back(UnixDatagramMessage {
            data: buffer.to_vec(),
            sender_name: datagram.local_name.lock().clone(),
            sender_cred: SocketPeerCred {
                pid: get_current_process().lock().pid.0,
                uid: 0,
                gid: 0,
            },
        });
        drop(recv_queue);

        if let Some(owner) = peer_datagram.owner.lock().as_ref().and_then(Weak::upgrade) {
            wake_pollers(&owner, PollableEvent::CanBeRead);
        }
        wake_io();
        Ok(buffer.len())
    }

    pub fn write_socket_to_path(&self, buffer: &[u8], path: &str) -> SocketResult<usize> {
        match self.kind {
            UnixSocketKind::Datagram => self.write_datagram_socket(buffer, Some(path), false),
            UnixSocketKind::Stream | UnixSocketKind::SeqPacket => self.write_socket(buffer),
        }
    }

    pub fn write_socket(&self, buffer: &[u8]) -> SocketResult<usize> {
        self.write_socket_with_flags(buffer, false)
    }

    pub fn write_socket_with_flags(
        &self,
        buffer: &[u8],
        force_nonblocking: bool,
    ) -> SocketResult<usize> {
        let nonblocking = force_nonblocking || self.is_nonblocking();
        loop {
            match self.kind {
                UnixSocketKind::Datagram => {
                    let written = self.write_datagram_socket(buffer, None, force_nonblocking)?;
                    if written == 0 {
                        continue;
                    }
                    return Ok(written);
                }
                UnixSocketKind::Stream | UnixSocketKind::SeqPacket => {
                    let stream = match &*self.state.lock() {
                        UnixSocketState::Stream(stream) => stream.clone(),
                        _ => return Err(SocketError::InvalidArguments),
                    };

                    if *stream.write_shutdown.lock() {
                        return Err(SocketError::BrokenPipe);
                    }

                    let peer = stream
                        .peer
                        .lock()
                        .as_ref()
                        .and_then(Weak::upgrade)
                        .ok_or(SocketError::BrokenPipe)?;

                    if *peer.read_shutdown.lock() {
                        return Err(SocketError::BrokenPipe);
                    }

                    let mut recv_buf = peer.recv_buf.lock();
                    if recv_buf.len() < STREAM_RECV_CAPACITY {
                        let writable = STREAM_RECV_CAPACITY - recv_buf.len();
                        let write_len = buffer.len().min(writable);
                        recv_buf.extend(buffer[..write_len].iter().copied());
                        drop(recv_buf);

                        if let Some(owner) = peer.owner.lock().as_ref().and_then(Weak::upgrade) {
                            wake_pollers(&owner, PollableEvent::CanBeRead);
                        }
                        wake_io();
                        return Ok(write_len);
                    }
                    drop(recv_buf);

                    if nonblocking {
                        return Err(SocketError::TryAgain);
                    }

                    let current = prepare_block_current(BlockType::WakeRequired {
                        wake_type: WakeType::IO,
                        deadline: None,
                    });

                    let peer_gone = stream
                        .peer
                        .lock()
                        .as_ref()
                        .and_then(Weak::upgrade)
                        .is_none();
                    let peer_not_reading = *peer.read_shutdown.lock();
                    let room_available = peer.recv_buf.lock().len() < STREAM_RECV_CAPACITY;

                    if peer_gone || peer_not_reading || room_available {
                        cancel_block(&current);
                    } else {
                        finish_block_current();
                    }
                }
            }
        }
    }
}
