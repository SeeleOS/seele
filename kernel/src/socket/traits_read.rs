use alloc::sync::Weak;

use super::{
    STREAM_RECV_CAPACITY, SocketError, SocketResult, UnixSocketObject, UnixSocketState,
    self_ref::object_ref, wait::wait_for_object_event, wake_io, wake_pollers,
};
use crate::{
    misc::profile::{self, HotSyscallPhase},
    object::{error::ObjectError, traits::Readable},
    polling::event::PollableEvent,
};

impl Readable for UnixSocketObject {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, ObjectError> {
        self.read_socket(buffer).map_err(Into::into)
    }
}

impl UnixSocketObject {
    pub fn read_socket(&self, buffer: &mut [u8]) -> SocketResult<usize> {
        self.read_socket_with_flags(buffer, false)
    }

    pub fn read_socket_with_flags(
        &self,
        buffer: &mut [u8],
        force_nonblocking: bool,
    ) -> SocketResult<usize> {
        self.read_socket_with_flags_and_mode_internal(buffer, force_nonblocking, false, false)
    }

    pub fn read_socket_with_flags_and_mode(
        &self,
        buffer: &mut [u8],
        force_nonblocking: bool,
        peek: bool,
    ) -> SocketResult<usize> {
        self.read_socket_with_flags_and_mode_internal(buffer, force_nonblocking, peek, false)
    }

    pub fn recv_socket_with_flags_and_mode(
        &self,
        buffer: &mut [u8],
        force_nonblocking: bool,
        peek: bool,
    ) -> SocketResult<usize> {
        self.read_socket_with_flags_and_mode_internal(buffer, force_nonblocking, peek, true)
    }

    fn read_socket_with_flags_and_mode_internal(
        &self,
        buffer: &mut [u8],
        force_nonblocking: bool,
        peek: bool,
        allow_control_only: bool,
    ) -> SocketResult<usize> {
        let nonblocking = force_nonblocking || self.is_nonblocking();
        loop {
            let state = {
                let state = self.state.lock();
                match &*state {
                    UnixSocketState::Datagram(datagram) => Some((None, Some(datagram.clone()))),
                    UnixSocketState::Stream(stream) => Some((Some(stream.clone()), None)),
                    _ => None,
                }
            };

            match state {
                Some((None, Some(datagram))) => {
                    if *datagram.read_shutdown.lock() {
                        return Ok(0);
                    }

                    let phase_start = profile::scope_start();
                    let message = if peek {
                        datagram.recv_queue.lock().front().cloned()
                    } else {
                        datagram.recv_queue.lock().pop_front()
                    };
                    if let Some(message) = message {
                        *datagram.peer_cred.lock() = message.sender_cred;
                        *datagram.peer_name.lock() = message.sender_name;
                        *datagram.peer_rights.lock() = message.rights;
                        let read = buffer.len().min(message.data.len());
                        buffer[..read].copy_from_slice(&message.data[..read]);
                        profile::record_hot_syscall_phase(
                            HotSyscallPhase::ReadUnixDatagram,
                            profile::scope_start().saturating_sub(phase_start),
                        );
                        return Ok(read);
                    }
                    profile::record_hot_syscall_phase(
                        HotSyscallPhase::ReadUnixDatagram,
                        profile::scope_start().saturating_sub(phase_start),
                    );

                    if nonblocking {
                        return Err(SocketError::TryAgain);
                    }

                    if let Some(object) = object_ref(&self.self_ref) {
                        let object_ref = object as crate::object::misc::ObjectRef;
                        wait_for_object_event(object_ref, PollableEvent::CanBeRead);
                    }
                }
                Some((Some(stream), None)) => {
                    if *stream.read_shutdown.lock() {
                        return Ok(0);
                    }

                    if self.kind == super::UnixSocketKind::SeqPacket {
                        let packet_len = stream.pending_packets.lock().front().copied();
                        if let Some(packet_len) = packet_len {
                            let mut recv_buf = stream.recv_buf.lock();
                            if peek {
                                let phase_start = profile::scope_start();
                                let read = buffer.len().min(packet_len);
                                for (dst, src) in buffer.iter_mut().zip(recv_buf.iter().take(read))
                                {
                                    *dst = *src;
                                }
                                profile::record_hot_syscall_phase(
                                    HotSyscallPhase::ReadUnixSeqpacketPeek,
                                    profile::scope_start().saturating_sub(phase_start),
                                );
                                return Ok(read);
                            }

                            let phase_start = profile::scope_start();
                            let was_full = recv_buf.len() >= STREAM_RECV_CAPACITY;
                            let read = buffer.len().min(packet_len);
                            for (index, byte) in recv_buf.drain(..packet_len).enumerate() {
                                if index < read {
                                    buffer[index] = byte;
                                }
                            }
                            drop(recv_buf);
                            stream.pending_packets.lock().pop_front();

                            if was_full {
                                if let Some(peer) =
                                    stream.peer.lock().as_ref().and_then(Weak::upgrade)
                                    && let Some(owner) =
                                        peer.owner.lock().as_ref().and_then(Weak::upgrade)
                                {
                                    wake_pollers(&owner, PollableEvent::CanBeWritten);
                                }
                                wake_io();
                            }
                            profile::record_hot_syscall_phase(
                                HotSyscallPhase::ReadUnixSeqpacketDrain,
                                profile::scope_start().saturating_sub(phase_start),
                            );
                            return Ok(read);
                        }
                    }

                    let mut recv_buf = stream.recv_buf.lock();
                    if allow_control_only && recv_buf.is_empty() && stream.has_front_rights() {
                        return Ok(0);
                    }
                    if !recv_buf.is_empty() {
                        if peek {
                            let phase_start = profile::scope_start();
                            let read = buffer.len().min(recv_buf.len());
                            for (dst, src) in buffer.iter_mut().zip(recv_buf.iter().take(read)) {
                                *dst = *src;
                            }
                            profile::record_hot_syscall_phase(
                                HotSyscallPhase::ReadUnixStreamPeek,
                                profile::scope_start().saturating_sub(phase_start),
                            );
                            return Ok(read);
                        }

                        let phase_start = profile::scope_start();
                        let was_full = recv_buf.len() >= STREAM_RECV_CAPACITY;
                        let mut read = 0;
                        while read < buffer.len() {
                            match recv_buf.pop_front() {
                                Some(byte) => buffer[read] = byte,
                                None => break,
                            }
                            read += 1;
                        }
                        drop(recv_buf);

                        if was_full {
                            if let Some(peer) = stream.peer.lock().as_ref().and_then(Weak::upgrade)
                                && let Some(owner) =
                                    peer.owner.lock().as_ref().and_then(Weak::upgrade)
                            {
                                wake_pollers(&owner, PollableEvent::CanBeWritten);
                            }
                            wake_io();
                        }
                        profile::record_hot_syscall_phase(
                            HotSyscallPhase::ReadUnixStreamDrain,
                            profile::scope_start().saturating_sub(phase_start),
                        );
                        return Ok(read);
                    }
                    drop(recv_buf);

                    let peer_gone = stream
                        .peer
                        .lock()
                        .as_ref()
                        .and_then(Weak::upgrade)
                        .is_none();
                    if peer_gone || *stream.peer_write_closed.lock() {
                        return Ok(0);
                    }
                    if nonblocking {
                        return Err(SocketError::TryAgain);
                    }

                    if let Some(object) = object_ref(&self.self_ref) {
                        let object_ref = object as crate::object::misc::ObjectRef;
                        wait_for_object_event(object_ref, PollableEvent::CanBeRead);
                    }
                }
                _ => return Err(SocketError::InvalidArguments),
            }
        }
    }
}
