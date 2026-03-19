use crate::{object::misc::ObjectRef, polling::event::PollableEvent};

#[derive(Clone, Debug)]
pub struct PollerReadyEvent {
    // Copied from the matching PollerEntry so userspace can identify which registration woke.
    pub data: u64,
    pub event: PollableEvent,
    pub object: ObjectRef,
}

impl PollerReadyEvent {
    pub fn new(object: ObjectRef, event: PollableEvent, data: u64) -> Self {
        Self {
            data,
            event,
            object,
        }
    }
}
