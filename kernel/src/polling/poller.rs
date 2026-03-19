use alloc::{sync::Arc, vec::Vec};

use crate::{
    object::{Object, misc::ObjectRef},
    polling::event::Event,
};

#[derive(Debug)]
pub struct PollerObject {
    entries: Vec<PollerEntry>,
}

#[derive(Debug)]
struct PollerEntry {
    event: Event,
    object: ObjectRef,
}

impl PollerEntry {
    fn new(object: ObjectRef, event: Event) -> Self {
        Self { event, object }
    }
}

impl PollerObject {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, object: ObjectRef, event: Event) {
        self.entries.push(PollerEntry::new(object, event));
    }

    pub fn remove(&mut self, object: ObjectRef, event: Event) {
        let mut index = 0;
        let mut waiting_to_remove = Vec::new();

        for entry in &self.entries {
            if entry.event == event && Arc::ptr_eq(&entry.object, &object) {
                waiting_to_remove.push(index);
            }

            index += 1;
        }

        for ele in waiting_to_remove {
            self.entries.remove(ele);
        }
    }
}

impl Object for PollerObject {}
