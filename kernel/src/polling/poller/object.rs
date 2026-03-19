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

    pub fn register_obj(&self, object: ObjectRef, event: PollableEvent, data: u64) {
        self.entries
            .lock()
            .push(PollerEntry::new(object, event, data));
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
        let matching_entries: Vec<u64> = self
            .entries
            .lock()
            .iter()
            .filter(|entry| entry.event == event && Arc::ptr_eq(&entry.object, &object))
            .map(|entry| entry.data)
            .collect();

        let interested = !matching_entries.is_empty();

        if interested {
            let mut woken_events = self.woken_events.lock();
            for data in matching_entries {
                woken_events.push(PollerReadyEvent::new(object.clone(), event, data));
            }
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
