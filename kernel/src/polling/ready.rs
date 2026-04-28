use crate::{object::misc::ObjectRef, polling::event::PollableEvent};

#[derive(Clone, Debug)]
pub struct PollerReadyEvent {
    // Copied from the matching PollerEntry so userspace can identify which registration woke.
    pub data: u64,
    pub object: ObjectRef,
    pub event: PollableEvent,
    pub ready_bits: u32,
}

impl PollerReadyEvent {
    pub fn new(object: ObjectRef, event: PollableEvent, data: u64, ready_bits: u32) -> Self {
        Self {
            data,
            object,
            event,
            ready_bits,
        }
    }
}
