use crate::polling::event::PollableEvent;

crate::test!("polling event conversion helpers", || {
    pollable_event_u64_conversion_preserves_known_and_unknown_values();
});

fn pollable_event_u64_conversion_preserves_known_and_unknown_values() {
    assert_eq!(PollableEvent::from(0), PollableEvent::CanBeRead);
    assert_eq!(PollableEvent::from(1), PollableEvent::CanBeWritten);
    assert_eq!(PollableEvent::from(2), PollableEvent::Error);
    assert_eq!(PollableEvent::from(3), PollableEvent::Closed);
    assert_eq!(PollableEvent::from(99), PollableEvent::Other(99));
}
