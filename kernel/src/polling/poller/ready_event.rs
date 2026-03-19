use crate::{object::misc::ObjectRef, polling::event::PollableEvent};

#[derive(Clone, Debug)]
pub struct PollerReadyEvent {
    pub event: PollableEvent,
    pub object: ObjectRef,
}

impl PollerReadyEvent {
    pub fn new(object: ObjectRef, event: PollableEvent) -> Self {
        Self { event, object }
    }
}
