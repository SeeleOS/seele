use alloc::sync::Weak;

use super::{UnixSocketObject, UnixSocketState};
use crate::polling::{event::PollableEvent, object::Pollable};

impl Pollable for UnixSocketObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match &*self.state.lock() {
            UnixSocketState::Listener(listener) => {
                matches!(event, PollableEvent::CanBeRead) && !listener.pending.lock().is_empty()
            }
            UnixSocketState::Stream(stream) => match event {
                PollableEvent::CanBeRead => {
                    !stream.recv_buf.lock().is_empty()
                        || *stream.write_closed.lock()
                        || stream
                            .peer
                            .lock()
                            .as_ref()
                            .and_then(Weak::upgrade)
                            .is_none()
                }
                PollableEvent::CanBeWritten => stream
                    .peer
                    .lock()
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some(),
                PollableEvent::Closed => {
                    *stream.write_closed.lock()
                        || stream
                            .peer
                            .lock()
                            .as_ref()
                            .and_then(Weak::upgrade)
                            .is_none()
                }
                _ => false,
            },
            _ => false,
        }
    }
}
