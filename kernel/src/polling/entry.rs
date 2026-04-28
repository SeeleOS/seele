use crate::{object::misc::ObjectRef, polling::event::PollableEvent};

#[derive(Debug)]
pub struct PollerEntry {
    // User-provided payload from epoll_event.data. It should be returned unchanged on wake.
    pub data: u64,
    pub object: ObjectRef,
    pub event: PollableEvent,
    pub ready_bits: u32,
    pub oneshot: bool,
    pub enabled: bool,
}

impl PollerEntry {
    pub fn new(
        object: ObjectRef,
        event: PollableEvent,
        data: u64,
        ready_bits: u32,
        oneshot: bool,
    ) -> Self {
        Self {
            data,
            object,
            event,
            ready_bits,
            oneshot,
            enabled: true,
        }
    }
}
