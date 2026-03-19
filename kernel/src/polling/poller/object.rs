use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    impl_cast_function_non_trait,
    object::{Object, misc::ObjectRef, tty_device::is_tty_readable},
    polling::{
        event::PollableEvent,
        poller::{PollerEntry, PollerReadyEvent},
    },
};

#[derive(Debug)]
pub struct PollerObject {
    // Registered objects that will notify the poller when an event is triggered
    pub entries: Mutex<Vec<PollerEntry>>,
    // Events that are triggered. Will be processed by caller (poll_wait syscall).
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

    // Checks for all matching entries that should be woken, and pushes them to the woken_events
    pub fn push_woken_event(&self, object: ObjectRef, event: PollableEvent) -> bool {
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

    fn is_entry_ready(entry: &PollerEntry) -> bool {
        matches!(entry.event, PollableEvent::CanBeRead) && is_tty_readable(&entry.object)
    }

    // Pushes the events that are already ready and dont need waiting.
    pub fn push_already_ready_events(&self) -> bool {
        let ready_entries: Vec<(ObjectRef, PollableEvent, u64)> = self
            .entries
            .lock()
            .iter()
            .filter(|entry| Self::is_entry_ready(entry))
            .map(|entry| (entry.object.clone(), entry.event, entry.data))
            .collect();

        let has_ready = !ready_entries.is_empty();

        if has_ready {
            let mut woken_events = self.woken_events.lock();
            for (object, event, data) in ready_entries {
                woken_events.push(PollerReadyEvent::new(object, event, data));
            }
        }

        has_ready
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
