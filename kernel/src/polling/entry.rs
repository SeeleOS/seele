use crate::{object::misc::ObjectRef, polling::event::PollableEvent};

#[derive(Debug)]
pub struct PollerEntry {
    // User-provided payload from epoll_event.data. It should be returned unchanged on wake.
    pub data: u64,
    pub event: PollableEvent,
    pub object: ObjectRef,
}

impl PollerEntry {
    pub fn new(object: ObjectRef, event: PollableEvent, data: u64) -> Self {
        Self {
            data,
            event,
            object,
        }
    }
}
