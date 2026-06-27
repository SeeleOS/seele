use alloc::{sync::Weak, vec::Vec};

use super::{
    DATAGRAM_RECV_CAPACITY, PendingRights, STREAM_RECV_CAPACITY, SocketError, SocketResult,
    UNIX_SOCKET_REGISTRY, UnixDatagramMessage, UnixSocketKind, UnixSocketObject,
    UnixSocketRegistryEntry, UnixSocketState, bind::socket_registry_key, current_socket_peer_cred,
    self_ref::object_ref, wait::wait_for_object_event, wake_io, wake_pollers,
};
use crate::{
    object::{error::ObjectError, misc::ObjectRef, traits::Writable},
    polling::event::PollableEvent,
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
        rights: Vec<ObjectRef>,
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
            let target_key =
                socket_registry_key(target_path).ok_or(SocketError::ConnectionRefused)?;
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
            let peer_key = socket_registry_key(&peer_name).ok_or(SocketError::ConnectionRefused)?;
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

        if let Some(program) = peer.attached_bpf.lock().clone() {
            program
                .run_socket_filter(buffer)
                .map_err(|_| SocketError::InvalidArguments)?;
        }

        loop {
            let mut recv_queue = peer_datagram.recv_queue.lock();
            if recv_queue.len() >= DATAGRAM_RECV_CAPACITY {
                drop(recv_queue);
                if nonblocking {
                    return Err(SocketError::TryAgain);
                }

                if let Some(owner) = peer_datagram.owner.lock().as_ref().and_then(Weak::upgrade) {
                    let object_ref = owner as crate::object::misc::ObjectRef;
                    wait_for_object_event(object_ref, PollableEvent::CanBeWritten);
                }
                continue;
            }

            recv_queue.push_back(UnixDatagramMessage {
                data: buffer.to_vec(),
                sender_name: datagram.local_name.lock().clone(),
                sender_cred: current_socket_peer_cred(),
                rights: rights.clone(),
            });
            drop(recv_queue);

            if let Some(owner) = peer_datagram.owner.lock().as_ref().and_then(Weak::upgrade) {
                wake_pollers(&owner, PollableEvent::CanBeRead);
            }
            wake_io();
            return Ok(buffer.len());
        }
    }

    pub fn write_socket_to_path_with_rights(
        &self,
        buffer: &[u8],
        path: &str,
        force_nonblocking: bool,
        rights: Vec<ObjectRef>,
    ) -> SocketResult<usize> {
        match self.kind {
            UnixSocketKind::Datagram => {
                self.write_datagram_socket(buffer, Some(path), force_nonblocking, rights)
            }
            UnixSocketKind::Stream | UnixSocketKind::SeqPacket => self.write_socket(buffer),
        }
    }

    pub fn write_socket_to_path(&self, buffer: &[u8], path: &str) -> SocketResult<usize> {
        self.write_socket_to_path_with_rights(buffer, path, false, Vec::new())
    }

    pub fn write_socket(&self, buffer: &[u8]) -> SocketResult<usize> {
        self.write_socket_with_flags(buffer, false)
    }

    pub fn write_socket_with_rights(
        &self,
        buffer: &[u8],
        force_nonblocking: bool,
        rights: Vec<ObjectRef>,
    ) -> SocketResult<usize> {
        let nonblocking = force_nonblocking || self.is_nonblocking();
        loop {
            match self.kind {
                UnixSocketKind::Datagram => {
                    let written = self.write_datagram_socket(
                        buffer,
                        None,
                        force_nonblocking,
                        rights.clone(),
                    )?;
                    return Ok(written);
                }
                UnixSocketKind::Stream | UnixSocketKind::SeqPacket => {
                    if buffer.is_empty() && rights.is_empty() {
                        return Ok(0);
                    }

                    let stream = {
                        let state = self.state.lock();
                        match &*state {
                            UnixSocketState::Stream(stream) => stream.clone(),
                            _ => return Err(SocketError::InvalidArguments),
                        }
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
                    let byte_offset = recv_buf.len();
                    let write_len = match self.kind {
                        UnixSocketKind::Stream => {
                            let available = STREAM_RECV_CAPACITY.saturating_sub(recv_buf.len());
                            buffer.len().min(available)
                        }
                        UnixSocketKind::SeqPacket => buffer.len(),
                        UnixSocketKind::Datagram => unreachable!(),
                    };
                    if self.kind == UnixSocketKind::SeqPacket && write_len > STREAM_RECV_CAPACITY {
                        return Err(SocketError::InvalidArguments);
                    }
                    let has_control_only_stream_rights = self.kind == UnixSocketKind::Stream
                        && buffer.is_empty()
                        && !rights.is_empty();
                    if write_len > 0 || has_control_only_stream_rights {
                        recv_buf.extend(buffer[..write_len].iter().copied());
                        if self.kind == UnixSocketKind::SeqPacket {
                            peer.pending_packets.lock().push_back(write_len);
                        }
                        if !rights.is_empty() {
                            peer.pending_rights.lock().push_back(PendingRights {
                                byte_offset,
                                rights,
                            });
                        }
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

                    if let Some(object) = object_ref(&self.self_ref) {
                        let object_ref = object as crate::object::misc::ObjectRef;
                        wait_for_object_event(object_ref, PollableEvent::CanBeWritten);
                    }
                }
            }
        }
    }

    pub fn write_socket_with_flags(
        &self,
        buffer: &[u8],
        force_nonblocking: bool,
    ) -> SocketResult<usize> {
        self.write_socket_with_rights(buffer, force_nonblocking, Vec::new())
    }
}
