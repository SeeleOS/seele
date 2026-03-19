use crate::{object::misc::ObjectRef, polling::event::PollableEvent};

#[derive(Debug)]
pub struct PollerEntry {
    pub event: PollableEvent,
    pub object: ObjectRef,
}

impl PollerEntry {
    pub fn new(object: ObjectRef, event: PollableEvent) -> Self {
        Self { event, object }
    }
}
