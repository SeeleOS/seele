use alloc::sync::Weak;

use super::{DATAGRAM_RECV_CAPACITY, STREAM_RECV_CAPACITY, UnixSocketObject, UnixSocketState};
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
                PollableEvent::CanBeWritten => {
                    if *stream.write_shutdown.lock() {
                        return false;
                    }

                    let Some(peer) = stream.peer.lock().as_ref().and_then(Weak::upgrade) else {
                        return false;
                    };

                    if *peer.read_shutdown.lock() {
                        return false;
                    }

                    peer.recv_buf.lock().len() < STREAM_RECV_CAPACITY
                }
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
            UnixSocketState::Datagram(datagram) => match event {
                PollableEvent::CanBeRead => !datagram.recv_queue.lock().is_empty(),
                PollableEvent::CanBeWritten => {
                    !*datagram.write_shutdown.lock()
                        && datagram.recv_queue.lock().len() < DATAGRAM_RECV_CAPACITY
                }
                PollableEvent::Closed => *datagram.write_shutdown.lock(),
                _ => false,
            },
            _ => false,
        }
    }
}
