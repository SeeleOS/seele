use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    impl_cast_function_non_trait,
    object::{Object, misc::ObjectRef},
    polling::{
        event::PollableEvent,
        poller::{PollerEntry, PollerReadyEvent},
    },
};

#[derive(Debug)]
pub struct PollerObject {
    pub entries: Mutex<Vec<PollerEntry>>,
    woken_events: Mutex<Vec<PollerReadyEvent>>,
}

impl PollerObject {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            woken_events: Mutex::new(Vec::new()),
        }
    }

    pub fn register_obj(&self, object: ObjectRef, event: PollableEvent) {
        self.entries.lock().push(PollerEntry::new(object, event));
    }

    pub fn unregister_obj(&self, object: ObjectRef, event: PollableEvent) {
        let mut waiting_to_remove = Vec::new();

        for (i, entry) in self.entries.lock().iter().enumerate() {
            if entry.event == event && Arc::ptr_eq(&entry.object, &object) {
                waiting_to_remove.push(i);
            }
        }

        for ele in waiting_to_remove.into_iter().rev() {
            self.entries.lock().remove(ele);
        }
    }

    pub fn queue_woken_event(&self, object: ObjectRef, event: PollableEvent) -> bool {
        let interested = self
            .entries
            .lock()
            .iter()
            .any(|entry| entry.event == event && Arc::ptr_eq(&entry.object, &object));

        if interested {
            self.woken_events
                .lock()
                .push(PollerReadyEvent::new(object, event));
        }

        interested
    }

    pub fn has_woken_events(&self) -> bool {
        !self.woken_events.lock().is_empty()
    }

    pub fn take_woken_events(&self, maxevents: usize) -> Vec<PollerReadyEvent> {
        let mut woken_events = self.woken_events.lock();
        let count = woken_events.len().min(maxevents);
        woken_events.drain(..count).collect()
    }
}

impl Object for PollerObject {
    impl_cast_function_non_trait!(poller, PollerObject);
}
