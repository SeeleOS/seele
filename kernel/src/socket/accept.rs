use alloc::sync::Arc;

use super::{SocketError, SocketResult, UnixSocketObject, UnixSocketState};
use crate::{
    process::manager::get_current_process,
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

impl UnixSocketObject {
    pub fn accept(self: &Arc<Self>) -> SocketResult<usize> {
        loop {
            let listener = match &*self.state.lock() {
                UnixSocketState::Listener(listener) => listener.clone(),
                _ => return Err(SocketError::InvalidArguments),
            };

            if let Some(socket) = listener.pending.lock().pop_front() {
                return Ok(get_current_process().lock().push_object(socket));
            }

            if self.is_nonblocking() {
                return Err(SocketError::TryAgain);
            }

            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });

            if !listener.pending.lock().is_empty() {
                cancel_block(&current);
            } else {
                finish_block_current();
            }
        }
    }
}
