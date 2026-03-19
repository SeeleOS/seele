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
}

impl Object for PollerObject {}
