use crate::{
    object::{misc::ObjectRef, tty_device::is_tty_readable},
    polling::{PollerEntry, PollerObject, PollerReadyEvent, event::PollableEvent},
};

impl PollerObject {
    // Checks for all matching entries that should be woken, and pushes them to woken_events.
    pub fn push_woken_event(&self, object: ObjectRef, event: PollableEvent) -> bool {
        let matching_entries: alloc::vec::Vec<u64> = self
            .entries
            .lock()
            .iter()
            .filter(|entry| {
                entry.event == event && alloc::sync::Arc::ptr_eq(&entry.object, &object)
            })
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

    // Pushes the events that are already ready and do not need waiting.
    pub fn push_already_ready_events(&self) -> bool {
        let ready_entries: alloc::vec::Vec<_> = self
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

    pub fn take_woken_events(&self, maxevents: usize) -> alloc::vec::Vec<PollerReadyEvent> {
        let mut woken_events = self.woken_events.lock();
        let count = woken_events.len().min(maxevents);
        woken_events.drain(..count).collect()
    }
}
