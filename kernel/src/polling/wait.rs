use alloc::sync::Arc;

use crate::{
    object::{Object, error::ObjectError, misc::ObjectRef},
    polling::{PollerObject, event::PollableEvent},
    process::manager::get_current_process,
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

pub fn wait_for_object_event(object: ObjectRef, event: PollableEvent) {
    let _ = wait_for_object_event_interruptible(object, event);
}

pub fn wait_for_object_event_interruptible(
    object: ObjectRef,
    event: PollableEvent,
) -> Result<(), ObjectError> {
    let poller = PollerObject::new();
    poller.register_obj(object, event, 0);

    poller.push_already_ready_events();
    if poller.has_woken_events() {
        let _ = poller.take_woken_events(1);
        return Ok(());
    }

    if has_pending_signal() {
        return Err(ObjectError::Interrupted);
    }

    let _guard = InterruptibleObjectWaitGuard::new();
    let poller_ref: Arc<dyn Object> = poller.clone();
    let current = prepare_block_current(BlockType::WakeRequired {
        wake_type: WakeType::Poller(poller_ref),
        deadline: None,
    });

    if !poller.has_woken_events() {
        poller.push_already_ready_events();
    }

    let result = if poller.has_woken_events() {
        cancel_block(&current);
        Ok(())
    } else {
        finish_block_current();
        if take_signal_interrupt() || has_pending_signal() {
            Err(ObjectError::Interrupted)
        } else {
            Ok(())
        }
    };

    let _ = poller.take_woken_events(1);
    result
}

fn has_pending_signal() -> bool {
    !get_current_process().lock().pending_signals.is_empty()
        || !crate::thread::get_current_thread()
            .lock()
            .pending_signals
            .is_empty()
}

fn take_signal_interrupt() -> bool {
    let current = crate::thread::get_current_thread();
    let mut current = current.lock();
    let interrupted = current.interrupted_by_signal;
    current.interrupted_by_signal = false;
    interrupted
}

struct InterruptibleObjectWaitGuard;

impl InterruptibleObjectWaitGuard {
    fn new() -> Self {
        crate::thread::get_current_thread()
            .lock()
            .interruptible_wait_active = true;
        Self
    }
}

impl Drop for InterruptibleObjectWaitGuard {
    fn drop(&mut self) {
        crate::thread::get_current_thread()
            .lock()
            .interruptible_wait_active = false;
    }
}
