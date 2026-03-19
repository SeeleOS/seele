use crate::multitasking::thread::yielding::{BlockType, WakeType, block_current};
use crate::object::misc::get_object_current_process;
use crate::polling::event::PollableEvent;
use crate::systemcall::numbers::*;
use crate::systemcall::utils::SyscallImpl;
use crate::systemcall::error::SyscallError;
use alloc::sync::Arc;

use crate::{
    define_syscall, multitasking::process::manager::get_current_process,
    polling::poller::PollerObject,
};

#[repr(C)]
struct RawPollerEvent {
    events: u32,
    _pad: u32,
    data: u64,
}

fn pollable_event_to_linux_bits(event: PollableEvent) -> u32 {
    match event {
        PollableEvent::CanBeRead => 0x001,
        PollableEvent::CanBeWritten => 0x004,
        PollableEvent::Error => 0x008,
        PollableEvent::Closed => 0x010,
        PollableEvent::Other(bits) => bits as u32,
    }
}

fn current_process_object_id(target: &Arc<dyn crate::object::Object>) -> Option<u64> {
    let process = get_current_process();
    let process = process.lock();

    process
        .objects
        .iter()
        .enumerate()
        .find_map(|(index, object)| {
            let object = object.as_ref()?;
            if Arc::ptr_eq(object, target) {
                Some(index as u64)
            } else {
                None
            }
        })
}

define_syscall!(CreatePoller, {
    let process = get_current_process();
    let objects = &mut process.lock().objects;

    objects.push(Some(Arc::new(PollerObject::new())));

    Ok(objects.len() - 1)
});

define_syscall!(PollerAdd, |poller: u64, target_object: u64, event: u64| {
    get_object_current_process(poller)
        .ok_or(SyscallError::BadFileDescriptor)?
        .as_poller()
        .ok_or(SyscallError::InvalidArguments)?
        .register_obj(
            get_object_current_process(target_object).ok_or(SyscallError::BadFileDescriptor)?,
            PollableEvent::from(event),
        );

    Ok(0)
});

define_syscall!(PollerRemove, |poller: u64,
                               target_object: u64,
                               event: u64| {
    get_object_current_process(poller)
        .ok_or(SyscallError::BadFileDescriptor)?
        .as_poller()
        .ok_or(SyscallError::InvalidArguments)?
        .unregister_obj(
            get_object_current_process(target_object).ok_or(SyscallError::BadFileDescriptor)?,
            PollableEvent::from(event),
        );

    Ok(0)
});

define_syscall!(PollerWait, |poller: u64, events_ptr: *mut u8, maxevents: usize| {
    if maxevents == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let poller = get_object_current_process(poller)
        .ok_or(SyscallError::BadFileDescriptor)?
        .as_poller()
        .ok_or(SyscallError::InvalidArguments)?;

    if !poller.has_ready_events() {
        let poller_ref: Arc<dyn crate::object::Object> = poller.clone();
        block_current(BlockType::WakeRequired(WakeType::Poller(poller_ref)));
    }

    let ready_events = poller.take_ready_events(maxevents);
    let events_ptr = events_ptr.cast::<RawPollerEvent>();

    if !events_ptr.is_null() {
        for (index, ready) in ready_events.iter().enumerate() {
            let Some(object_id) = current_process_object_id(&ready.object) else {
                continue;
            };

            unsafe {
                events_ptr.add(index).write(RawPollerEvent {
                    events: pollable_event_to_linux_bits(ready.event),
                    _pad: 0,
                    data: object_id,
                });
            }
        }
    }

    Ok(ready_events.len())
});
