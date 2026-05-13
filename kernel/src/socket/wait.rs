use alloc::sync::Arc;

use crate::{
    object::{Object, misc::ObjectRef},
    polling::{PollerObject, event::PollableEvent},
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

pub(crate) fn wait_for_object_event(object: ObjectRef, event: PollableEvent) {
    let poller = PollerObject::new();
    poller.register_obj(object, event, 0);
    poller.push_already_ready_events();
    if poller.has_woken_events() {
        let _ = poller.take_woken_events(1);
        return;
    }

    let poller_ref: Arc<dyn Object> = poller.clone();
    let current = prepare_block_current(BlockType::WakeRequired {
        wake_type: WakeType::Poller(poller_ref),
        deadline: None,
    });

    if !poller.has_woken_events() {
        poller.push_already_ready_events();
    }

    if poller.has_woken_events() {
        cancel_block(&current);
    } else {
        finish_block_current();
    }

    let _ = poller.take_woken_events(1);
}
