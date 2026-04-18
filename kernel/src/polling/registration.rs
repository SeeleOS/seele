use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::Mutex;

use crate::{
    object::misc::ObjectRef,
    polling::{PollerEntry, PollerObject, event::PollableEvent},
    s_println,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RegistryKey {
    object: usize,
    event: u64,
}

#[derive(Default)]
struct PollRegistry {
    watchers: BTreeMap<RegistryKey, Vec<Weak<PollerObject>>>,
}

lazy_static::lazy_static! {
    static ref POLL_REGISTRY: Mutex<PollRegistry> = Mutex::new(PollRegistry::default());
}

fn object_key(object: &ObjectRef) -> usize {
    Arc::as_ptr(object) as *const () as usize
}

fn should_trace_object(object: &ObjectRef) -> bool {
    matches!(
        object.debug_name(),
        "evdev-client" | "kernel::polling::object::PollerObject"
    )
}

fn event_key(event: PollableEvent) -> u64 {
    match event {
        PollableEvent::CanBeRead => 0,
        PollableEvent::CanBeWritten => 1,
        PollableEvent::Error => 2,
        PollableEvent::Closed => 3,
        PollableEvent::Other(bits) => bits,
    }
}

fn registry_key(object: &ObjectRef, event: PollableEvent) -> RegistryKey {
    RegistryKey {
        object: object_key(object),
        event: event_key(event),
    }
}

fn same_poller(left: &Weak<PollerObject>, right: &Arc<PollerObject>) -> bool {
    left.upgrade()
        .is_some_and(|poller| Arc::ptr_eq(&poller, right))
}

fn register_interest(poller: &Arc<PollerObject>, object: &ObjectRef, event: PollableEvent) {
    let mut registry = POLL_REGISTRY.lock();
    let watchers = registry
        .watchers
        .entry(registry_key(object, event))
        .or_default();
    watchers.retain(|watcher| watcher.strong_count() > 0);
    if !watchers.iter().any(|watcher| same_poller(watcher, poller)) {
        watchers.push(Arc::downgrade(poller));
    }
}

fn unregister_interest(poller: &Arc<PollerObject>, object: &ObjectRef, event: PollableEvent) {
    let mut registry = POLL_REGISTRY.lock();
    let key = registry_key(object, event);
    let Some(watchers) = registry.watchers.get_mut(&key) else {
        return;
    };
    watchers.retain(|watcher| watcher.strong_count() > 0 && !same_poller(watcher, poller));
    if watchers.is_empty() {
        registry.watchers.remove(&key);
    }
}

fn interested_pollers(object: &ObjectRef, event: PollableEvent) -> Vec<Arc<PollerObject>> {
    let mut registry = POLL_REGISTRY.lock();
    let Some(watchers) = registry.watchers.get_mut(&registry_key(object, event)) else {
        return Vec::new();
    };

    let mut pollers = Vec::new();
    watchers.retain(|watcher| {
        if let Some(poller) = watcher.upgrade() {
            pollers.push(poller);
            true
        } else {
            false
        }
    });

    pollers
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PollWakeResult {
    pub interested: bool,
    pub became_readable: bool,
}

pub fn notify_pollers(object: ObjectRef, event: PollableEvent) -> Vec<ObjectRef> {
    let mut queue = VecDeque::from([(object, event)]);
    let mut visited = BTreeSet::new();
    let mut affected = Vec::new();
    let mut affected_keys = BTreeSet::new();

    while let Some((current_object, current_event)) = queue.pop_front() {
        if !visited.insert(registry_key(&current_object, current_event)) {
            continue;
        }

        let pollers = interested_pollers(&current_object, current_event);
        if !pollers.is_empty() && should_trace_object(&current_object) {
            s_println!(
                "poll: notify object={} ptr={:#x} event={:?} watchers={}",
                current_object.debug_name(),
                object_key(&current_object),
                current_event,
                pollers.len()
            );
        }

        for poller in pollers {
            let result = poller.push_woken_event(current_object.clone(), current_event);
            if !result.interested {
                continue;
            }

            let Some(poller_object) = poller.self_object() else {
                continue;
            };
            let poller_key = object_key(&poller_object);
            if affected_keys.insert(poller_key) {
                affected.push(poller_object.clone());
            }
            if result.became_readable {
                queue.push_back((poller_object, PollableEvent::CanBeRead));
            }
        }
    }

    affected
}

impl PollerObject {
    pub fn register_obj(&self, object: ObjectRef, event: PollableEvent, data: u64) {
        let mut entries = self.entries.lock();
        let is_new_entry = if let Some(existing) = entries
            .iter_mut()
            .find(|entry| entry.event == event && Arc::ptr_eq(&entry.object, &object))
        {
            existing.data = data;
            false
        } else {
            entries.push(PollerEntry::new(object.clone(), event, data));
            true
        };
        drop(entries);

        if is_new_entry && let Some(poller) = self.self_poller() {
            register_interest(&poller, &object, event);
        }

        if should_trace_object(&object) {
            let poller_ptr = self as *const Self as usize;
            s_println!(
                "poll: register poller={:#x} object={} ptr={:#x} event={:?} data={:#x} new={}",
                poller_ptr,
                object.debug_name(),
                object_key(&object),
                event,
                data,
                is_new_entry
            );
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

        if let Some(poller) = self.self_poller() {
            unregister_interest(&poller, &object, event);
        }

        if should_trace_object(&object) {
            let poller_ptr = self as *const Self as usize;
            s_println!(
                "poll: unregister poller={:#x} object={} ptr={:#x} event={:?}",
                poller_ptr,
                object.debug_name(),
                object_key(&object),
                event
            );
        }

        self.woken_events
            .lock()
            .retain(|ready| !(ready.event == event && Arc::ptr_eq(&ready.object, &object)));
    }
}
