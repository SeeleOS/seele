use alloc::sync::Arc;

use super::UnixSocketObject;
use crate::{object::misc::ObjectRef, polling::event::PollableEvent, thread::THREAD_MANAGER};

pub(crate) fn wake_io() {
    if let Some(manager) = THREAD_MANAGER.get() {
        manager.lock().wake_io();
    }
}

pub(crate) fn wake_pollers(target: &Arc<UnixSocketObject>, event: PollableEvent) {
    if let Some(manager) = THREAD_MANAGER.get() {
        let object_ref: ObjectRef = target.clone();
        manager.lock().wake_poller(object_ref, event);
    }
}
