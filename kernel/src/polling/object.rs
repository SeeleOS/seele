use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::Mutex;

use crate::{
    impl_cast_function, impl_cast_function_non_trait,
    object::Object,
    polling::{PollerEntry, PollerReadyEvent, event::PollableEvent},
};

#[derive(Debug)]
pub struct PollerObject {
    // Registered objects that will notify the poller when an event is triggered.
    pub entries: Mutex<Vec<PollerEntry>>,
    // Events collected for the next poller_wait call.
    pub woken_events: Mutex<Vec<PollerReadyEvent>>,
    self_ref: Mutex<Option<Weak<PollerObject>>>,
}

impl PollerObject {
    pub fn new() -> Arc<Self> {
        let poller = Arc::new(Self {
            entries: Mutex::new(Vec::new()),
            woken_events: Mutex::new(Vec::new()),
            self_ref: Mutex::new(None),
        });
        *poller.self_ref.lock() = Some(Arc::downgrade(&poller));
        poller
    }

    pub fn self_object(&self) -> Option<crate::object::misc::ObjectRef> {
        self.self_ref
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|poller| poller as crate::object::misc::ObjectRef)
    }

    pub fn self_poller(&self) -> Option<Arc<Self>> {
        self.self_ref.lock().as_ref().and_then(Weak::upgrade)
    }
}

impl crate::polling::object::Pollable for PollerObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead) && !self.woken_events.lock().is_empty()
    }
}

impl Object for PollerObject {
    impl_cast_function!("pollable", crate::polling::object::Pollable);
    impl_cast_function_non_trait!("poller", PollerObject);
}

pub trait Pollable: Object {
    fn is_event_ready(&self, event: PollableEvent) -> bool;
}
