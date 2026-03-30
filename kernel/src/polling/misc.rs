use crate::{object::Object, polling::event::PollableEvent};

pub trait Pollable: Object {
    fn is_event_ready(&self, event: PollableEvent) -> bool;
}
