use alloc::{sync::Arc, vec::Vec};

use crate::{
    object::misc::ObjectRef,
    polling::{PollerEntry, PollerObject, event::PollableEvent},
};

impl PollerObject {
    pub fn register_obj(&self, object: ObjectRef, event: PollableEvent, data: u64) {
        let mut entries = self.entries.lock();
        if let Some(existing) = entries
            .iter_mut()
            .find(|entry| entry.event == event && Arc::ptr_eq(&entry.object, &object))
        {
            existing.data = data;
        } else {
            entries.push(PollerEntry::new(object.clone(), event, data));
        }

        self.woken_events
            .lock()
            .retain(|ready| !(ready.event == event && Arc::ptr_eq(&ready.object, &object)));
    }

    pub fn unregister_obj(&self, object: ObjectRef, event: PollableEvent) {
        let mut waiting_to_remove = Vec::new();

        for (index, entry) in self.entries.lock().iter().enumerate() {
            if entry.event == event && Arc::ptr_eq(&entry.object, &object) {
                waiting_to_remove.push(index);
            }
        }

        {
            let mut entries = self.entries.lock();
            for index in waiting_to_remove.into_iter().rev() {
                entries.remove(index);
            }
        }

        self.woken_events.lock().retain(|ready| {
            !(ready.event == event && Arc::ptr_eq(&ready.object, &object))
        });
    }
}
