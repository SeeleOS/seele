use alloc::sync::Weak;

use super::{
    STREAM_RECV_CAPACITY, SocketError, SocketResult, UnixSocketObject, UnixSocketState, wake_io,
    wake_pollers,
};
use crate::{
    object::{error::ObjectError, traits::Writable},
    polling::event::PollableEvent,
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
