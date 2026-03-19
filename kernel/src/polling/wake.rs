use alloc::sync::Arc;

use crate::{
    object::misc::ObjectRef,
    polling::{event::PollableEvent, poller::PollerObject},
};

impl PollerObject {
    pub fn wake(&self, obj: ObjectRef, event: PollableEvent) {
        self.entries
            .lock()
            .retain(|f| !(f.event == event && Arc::ptr_eq(&f.object, &obj)));
    }
}
