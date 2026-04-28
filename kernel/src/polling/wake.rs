use alloc::{collections::BTreeSet, sync::Arc, vec::Vec};

use crate::{
    object::misc::ObjectRef,
    polling::{
        PollerEntry, PollerObject, PollerReadyEvent, event::PollableEvent,
        registration::PollWakeResult,
    },
};

impl PollerObject {
    fn queue_ready_event(
        woken_events: &mut Vec<PollerReadyEvent>,
        object: &ObjectRef,
        event: PollableEvent,
        data: u64,
        ready_bits: u32,
    ) -> bool {
        if let Some(existing) = woken_events
            .iter_mut()
            .find(|ready| ready.data == data && Arc::ptr_eq(&ready.object, object))
        {
            let was_empty = existing.ready_bits == 0;
            existing.ready_bits |= ready_bits;
            return was_empty && existing.ready_bits != 0;
        }

        woken_events.push(PollerReadyEvent::new(
            object.clone(),
            event,
            data,
            ready_bits,
        ));
        true
    }

    fn object_key(object: &ObjectRef) -> usize {
        Arc::as_ptr(object) as *const () as usize
    }

    // Checks for all matching entries that should be woken, and pushes them to woken_events.
    pub fn push_woken_event(&self, object: ObjectRef, event: PollableEvent) -> PollWakeResult {
        let matching_entries: Vec<(u64, u32, bool)> = self
            .entries
            .lock()
            .iter()
            .filter(|entry| {
                entry.enabled && entry.event == event && Arc::ptr_eq(&entry.object, &object)
            })
            .map(|entry| (entry.data, entry.ready_bits, entry.oneshot))
            .collect();

        let interested = !matching_entries.is_empty();
        let mut became_readable = false;
        let disable_oneshot = matching_entries.iter().any(|(_, _, oneshot)| *oneshot);

        if interested {
            let mut woken_events = self.woken_events.lock();
            let was_empty = woken_events.is_empty();
            for (data, ready_bits, _) in matching_entries {
                let _ =
                    Self::queue_ready_event(&mut woken_events, &object, event, data, ready_bits);
            }
            became_readable = was_empty && !woken_events.is_empty();
        }
        if disable_oneshot {
            self.disable_oneshot_entries(&object);
        }

        PollWakeResult {
            interested,
            became_readable,
        }
    }

    fn is_entry_ready(entry: &PollerEntry) -> bool {
        if let Ok(object) = entry.object.clone().as_pollable() {
            return object.is_event_ready(entry.event);
        }

        false
    }

    // Pushes the events that are already ready and do not need waiting.
    pub fn push_already_ready_events(&self) -> bool {
        let ready_entries: Vec<_> = self
            .entries
            .lock()
            .iter()
            .filter(|entry| entry.enabled && Self::is_entry_ready(entry))
            .map(|entry| {
                (
                    entry.object.clone(),
                    entry.event,
                    entry.data,
                    entry.ready_bits,
                    entry.oneshot,
                )
            })
            .collect();

        let has_ready = !ready_entries.is_empty();

        if has_ready {
            let mut woken_events = self.woken_events.lock();
            let mut disable_keys = BTreeSet::new();
            for (object, event, data, ready_bits, oneshot) in ready_entries {
                let _ =
                    Self::queue_ready_event(&mut woken_events, &object, event, data, ready_bits);
                if oneshot {
                    disable_keys.insert(Self::object_key(&object));
                }
            }
            drop(woken_events);

            if !disable_keys.is_empty() {
                for entry in self.entries.lock().iter_mut() {
                    if disable_keys.contains(&Self::object_key(&entry.object)) {
                        entry.enabled = false;
                    }
                }
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
