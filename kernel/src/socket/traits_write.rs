use alloc::sync::Weak;

use super::{
    DATAGRAM_RECV_CAPACITY, STREAM_RECV_CAPACITY, SocketError, SocketResult, UNIX_SOCKET_REGISTRY,
    UnixDatagramMessage, UnixSocketKind, UnixSocketObject, UnixSocketRegistryEntry,
    UnixSocketState, wake_io, wake_pollers,
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
    pub fn write_socket(&self, buffer: &[u8]) -> SocketResult<usize> {
        loop {
            match self.kind {
                UnixSocketKind::Datagram => {
                    let datagram = match &*self.state.lock() {
                        UnixSocketState::Datagram(datagram) => datagram.clone(),
                        _ => return Err(SocketError::InvalidArguments),
                    };

                    if *datagram.write_shutdown.lock() {
                        return Err(SocketError::BrokenPipe);
                    }

                    let peer =
                        if let Some(peer) = datagram.peer.lock().as_ref().and_then(Weak::upgrade) {
                            peer
                        } else {
                            let peer_name = datagram
                                .peer_name
                                .lock()
                                .clone()
                                .ok_or(SocketError::ConnectionRefused)?;
                            let registry = UNIX_SOCKET_REGISTRY.lock();
                            match registry.get(&peer_name) {
                                Some(UnixSocketRegistryEntry::Datagram(endpoint)) => {
                                    endpoint.upgrade().ok_or(SocketError::ConnectionRefused)?
                                }
                                _ => return Err(SocketError::ConnectionRefused),
                            }
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
                        if self.is_nonblocking() {
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
                        continue;
                    }

                    recv_queue.push_back(UnixDatagramMessage {
                        data: buffer.to_vec(),
                        sender_name: datagram.local_name.lock().clone(),
                        sender_cred: crate::socket::SocketPeerCred {
                            pid: get_current_process().lock().pid.0,
                            uid: 0,
                            gid: 0,
                        },
                    });
                    drop(recv_queue);

                    if let Some(owner) = peer_datagram.owner.lock().as_ref().and_then(Weak::upgrade)
                    {
                        wake_pollers(&owner, PollableEvent::CanBeRead);
                    }
                    wake_io();
                    return Ok(buffer.len());
                }
                UnixSocketKind::Stream => {
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

                    if self.is_nonblocking() {
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
