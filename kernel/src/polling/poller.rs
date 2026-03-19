use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    impl_cast_function_non_trait,
    multitasking::thread::yielding::{BlockType, block_current},
    object::{Object, misc::ObjectRef},
    polling::event::PollableEvent,
};

#[derive(Debug)]
pub struct PollerObject {
    entries: Mutex<Vec<PollerEntry>>,
}

#[derive(Debug)]
struct PollerEntry {
    event: PollableEvent,
    object: ObjectRef,
}

impl PollerEntry {
    fn new(object: ObjectRef, event: PollableEvent) -> Self {
        Self { event, object }
    }
}

impl PollerObject {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    pub fn add(&self, object: ObjectRef, event: PollableEvent) {
        self.entries.lock().push(PollerEntry::new(object, event));
    }

    pub fn remove(&self, object: ObjectRef, event: PollableEvent) {
        let mut waiting_to_remove = Vec::new();

        for (i, entry) in self.entries.lock().iter().enumerate() {
            if entry.event == event && Arc::ptr_eq(&entry.object, &object) {
                waiting_to_remove.push(i);
            }
        }

        for ele in waiting_to_remove {
            self.entries.lock().remove(ele);
        }
    }
}

impl Object for PollerObject {
    impl_cast_function_non_trait!(poller, PollerObject);
}
