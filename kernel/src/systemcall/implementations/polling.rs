use crate::object::misc::get_object_current_process;
use crate::polling::event::PollableEvent;
use crate::systemcall::numbers::*;
use crate::systemcall::utils::SyscallImpl;
use crate::systemcall::{error::SyscallError, implementations::objects};
use alloc::sync::Arc;
use x86_64::instructions::interrupts::enable;

use crate::{
    define_syscall, multitasking::process::manager::get_current_process,
    polling::poller::PollerObject,
};

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
        .add(
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
        .remove(
            get_object_current_process(target_object).ok_or(SyscallError::BadFileDescriptor)?,
            PollableEvent::from(event),
        );

    Ok(0)
});
