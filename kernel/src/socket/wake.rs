use alloc::sync::Arc;

use super::UnixSocketObject;
use crate::{
    object::misc::ObjectRef,
    polling::event::PollableEvent,
    thread::{try_with_thread_manager, yielding::wake_pollers_for_object},
};

pub(crate) fn wake_io() {
    let _ = try_with_thread_manager(|manager| manager.wake_io());
}

pub(crate) fn wake_pollers(target: &Arc<UnixSocketObject>, event: PollableEvent) {
    let object_ref: ObjectRef = target.clone();
    wake_pollers_for_object(object_ref, event);
}
