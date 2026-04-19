use alloc::sync::Weak;

use super::{
    STREAM_RECV_CAPACITY, SocketError, SocketResult, UnixSocketObject, UnixSocketState, wake_io,
    wake_pollers,
};
use crate::{
    object::{error::ObjectError, traits::Readable},
    polling::event::PollableEvent,
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

impl Readable for UnixSocketObject {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, ObjectError> {
        self.read_socket(buffer).map_err(Into::into)
    }
}

impl UnixSocketObject {
    pub fn read_socket(&self, buffer: &mut [u8]) -> SocketResult<usize> {
        loop {
            match &*self.state.lock() {
                UnixSocketState::Datagram(datagram) => {
                    if *datagram.read_shutdown.lock() {
                        return Ok(0);
                    }

                    let message = datagram.recv_queue.lock().pop_front();
                    if let Some(message) = message {
                        *datagram.peer_cred.lock() = message.sender_cred;
                        *datagram.peer_name.lock() = message.sender_name;
                        let read = buffer.len().min(message.data.len());
                        buffer[..read].copy_from_slice(&message.data[..read]);
                        return Ok(read);
                    }

                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }

                    let current = prepare_block_current(BlockType::WakeRequired {
                        wake_type: WakeType::IO,
                        deadline: None,
                    });

                    if !datagram.recv_queue.lock().is_empty() || *datagram.read_shutdown.lock() {
                        cancel_block(&current);
                    } else {
                        finish_block_current();
                    }
                }
                UnixSocketState::Stream(stream) => {
                    if *stream.read_shutdown.lock() {
                        return Ok(0);
                    }

                    let mut recv_buf = stream.recv_buf.lock();
                    if !recv_buf.is_empty() {
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
                        return Ok(read);
                    }
                    drop(recv_buf);

                    let peer_gone = stream
                        .peer
                        .lock()
                        .as_ref()
                        .and_then(Weak::upgrade)
                        .is_none();
                    if peer_gone || *stream.write_closed.lock() {
                        return Ok(0);
                    }
                    if self.is_nonblocking() {
                        return Err(SocketError::TryAgain);
                    }

                    let current = prepare_block_current(BlockType::WakeRequired {
                        wake_type: WakeType::IO,
                        deadline: None,
                    });

                    let ready_after_register = !stream.recv_buf.lock().is_empty()
                        || *stream.write_closed.lock()
                        || stream
                            .peer
                            .lock()
                            .as_ref()
                            .and_then(Weak::upgrade)
                            .is_none();

                    if ready_after_register {
                        cancel_block(&current);
                    } else {
                        finish_block_current();
                    }
                }
                _ => return Err(SocketError::InvalidArguments),
            }
        }
    }
}
