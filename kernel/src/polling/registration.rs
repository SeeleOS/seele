use alloc::{sync::Arc, vec::Vec};

use crate::{
    object::misc::ObjectRef,
    polling::{PollerEntry, PollerObject, event::PollableEvent},
};

impl PollerObject {
    pub fn register_obj(&self, object: ObjectRef, event: PollableEvent, data: u64) {
        self.entries
            .lock()
            .push(PollerEntry::new(object, event, data));
    }

    pub fn unregister_obj(&self, object: ObjectRef, event: PollableEvent) {
        let mut waiting_to_remove = Vec::new();

        for (index, entry) in self.entries.lock().iter().enumerate() {
            if entry.event == event && Arc::ptr_eq(&entry.object, &object) {
                waiting_to_remove.push(index);
            }
        }

        for index in waiting_to_remove.into_iter().rev() {
            self.entries.lock().remove(index);
        }
    }
}
