use alloc::sync::Weak;

use super::{SocketError, SocketResult, UnixSocketObject, UnixSocketState};
use crate::{
    object::{error::ObjectError, traits::Readable},
    thread::yielding::{BlockType, WakeType, block_current},
};

impl Readable for UnixSocketObject {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, ObjectError> {
        self.read_socket(buffer).map_err(Into::into)
    }
}

impl UnixSocketObject {
    pub fn read_socket(&self, buffer: &mut [u8]) -> SocketResult<usize> {
        loop {
            let stream = match &*self.state.lock() {
                UnixSocketState::Stream(stream) => stream.clone(),
                _ => return Err(SocketError::InvalidArguments),
            };

            if *stream.read_shutdown.lock() {
                return Ok(0);
            }

            let mut recv_buf = stream.recv_buf.lock();
            if !recv_buf.is_empty() {
                let mut read = 0;
                while read < buffer.len() {
                    match recv_buf.pop_front() {
                        Some(byte) => buffer[read] = byte,
                        None => break,
                    }
                    read += 1;
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
            block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });
        }
    }
}
