use alloc::sync::Arc;

use crate::{
    object::{FileFlags, Object},
    polling::{
        event::PollableEvent, notify_pollers, object::Pollable, poller::PollerObject,
        ready::PollerReadyEvent,
    },
};

crate::test!(
    pollable_event_conversion,
    "pollable event u64 conversion preserves known and unknown values",
    pollable_event_u64_conversion_preserves_known_and_unknown_values
);
crate::test!(
    poller_ready_event_constructor,
    "poller ready events preserve data event and ready bits",
    poller_ready_events_preserve_data_event_and_ready_bits
);
crate::test!(
    notify_pollers_only_wakes_became_readable_pollers,
    "notify_pollers ignores interested pollers with no ready events",
    notify_pollers_only_wakes_became_readable_pollers
);

#[derive(Debug)]
struct DummyObject;

impl Object for DummyObject {
    fn get_flags(self: Arc<Self>) -> crate::object::misc::ObjectResult<FileFlags> {
        Ok(FileFlags::empty())
    }
}

impl Pollable for DummyObject {
    fn is_event_ready(&self, _event: PollableEvent) -> bool {
        false
    }
}

fn pollable_event_u64_conversion_preserves_known_and_unknown_values() {
    assert_eq!(PollableEvent::from(0), PollableEvent::CanBeRead);
    assert_eq!(PollableEvent::from(1), PollableEvent::CanBeWritten);
    assert_eq!(PollableEvent::from(2), PollableEvent::Error);
    assert_eq!(PollableEvent::from(3), PollableEvent::Closed);
    assert_eq!(PollableEvent::from(4), PollableEvent::ReadClosed);
    assert_eq!(PollableEvent::from(99), PollableEvent::Other(99));
}

fn poller_ready_events_preserve_data_event_and_ready_bits() {
    let object: Arc<dyn Object> = Arc::new(DummyObject);
    let ready = PollerReadyEvent::new(object.clone(), PollableEvent::Error, 0xfeed, 0x11);

    assert_eq!(ready.data, 0xfeed);
    assert!(Arc::ptr_eq(&ready.object, &object));
    assert_eq!(ready.event, PollableEvent::Error);
    assert_eq!(ready.ready_bits, 0x11);
}

fn notify_pollers_only_wakes_became_readable_pollers() {
    let object: Arc<dyn Object> = Arc::new(DummyObject);
    let poller = PollerObject::new();
    poller.register_obj(object.clone(), PollableEvent::CanBeRead, 0x44);

    let affected = notify_pollers(object, PollableEvent::CanBeRead);

    assert!(affected.is_empty());
    assert!(!poller.has_woken_events());
}
