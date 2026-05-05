use alloc::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Weak},
    vec::Vec,
};
use spin::Mutex;

use crate::{
    object::misc::ObjectRef,
    polling::{PollerEntry, PollerObject, event::PollableEvent},
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

fn poller_key(poller: &Arc<PollerObject>) -> usize {
    Arc::as_ptr(poller) as *const () as usize
}

fn weak_poller_key(poller: &Weak<PollerObject>) -> usize {
    poller.as_ptr() as *const () as usize
}

fn same_poller_key(left: &Weak<PollerObject>, right_key: usize) -> bool {
    weak_poller_key(left) == right_key
}

fn register_interest(poller: &Arc<PollerObject>, object: &ObjectRef, event: PollableEvent) {
    let mut registry = POLL_REGISTRY.lock();
    let watchers = registry
        .watchers
        .entry(registry_key(object, event))
        .or_default();
    let key = poller_key(poller);
    watchers.retain(|watcher| watcher.strong_count() > 0);
    if !watchers.iter().any(|watcher| same_poller_key(watcher, key)) {
        watchers.push(Arc::downgrade(poller));
    }
}

fn unregister_interest(poller: &Arc<PollerObject>, object: &ObjectRef, event: PollableEvent) {
    let mut registry = POLL_REGISTRY.lock();
    let key = registry_key(object, event);
    let Some(watchers) = registry.watchers.get_mut(&key) else {
        return;
    };
    let poller_key = poller_key(poller);
    watchers.retain(|watcher| watcher.strong_count() > 0 && !same_poller_key(watcher, poller_key));
    if watchers.is_empty() {
        registry.watchers.remove(&key);
    }
}

pub(super) fn unregister_all_interests(poller_key: usize, entries: &[PollerEntry]) {
    let mut registry = POLL_REGISTRY.lock();

    for entry in entries {
        let key = registry_key(&entry.object, entry.event);
        let Some(watchers) = registry.watchers.get_mut(&key) else {
            continue;
        };
        watchers
            .retain(|watcher| watcher.strong_count() > 0 && !same_poller_key(watcher, poller_key));
        if watchers.is_empty() {
            registry.watchers.remove(&key);
        }
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
        self.register_obj_with_ready_bits(
            object,
            event,
            data,
            match event {
                PollableEvent::CanBeRead => 0x001,
                PollableEvent::CanBeWritten => 0x004,
                PollableEvent::Error => 0x008,
                PollableEvent::Closed => 0x010,
                PollableEvent::Other(bits) => bits as u32,
            },
            false,
        );
    }

    pub fn register_obj_with_ready_bits(
        &self,
        object: ObjectRef,
        event: PollableEvent,
        data: u64,
        ready_bits: u32,
        oneshot: bool,
    ) {
        let mut entries = self.entries.lock();
        let is_new_entry = if let Some(existing) = entries.iter_mut().find(|entry| {
            entry.event == event
                && entry.ready_bits == ready_bits
                && Arc::ptr_eq(&entry.object, &object)
        }) {
            existing.data = data;
            existing.oneshot = oneshot;
            existing.enabled = true;
            false
        } else {
            entries.push(PollerEntry::new(
                object.clone(),
                event,
                data,
                ready_bits,
                oneshot,
            ));
            true
        };
        drop(entries);

        if is_new_entry && let Some(poller) = self.self_poller() {
            register_interest(&poller, &object, event);
        }

        self.woken_events
            .lock()
            .retain(|ready| !Arc::ptr_eq(&ready.object, &object));
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

        self.woken_events
            .lock()
            .retain(|ready| !Arc::ptr_eq(&ready.object, &object));
    }

    pub fn disable_oneshot_entries(&self, object: &ObjectRef) {
        for entry in self.entries.lock().iter_mut() {
            if Arc::ptr_eq(&entry.object, object) {
                entry.enabled = false;
            }
        }
    }
}
