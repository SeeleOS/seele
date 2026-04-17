use crate::misc::time::Time;
use crate::object::misc::ObjectRef;
use crate::polling::event::PollableEvent;
use crate::systemcall::utils::SyscallImpl;
use crate::thread::yielding::{
    BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
};
use alloc::sync::Arc;

use crate::systemcall::utils::SyscallError;
use crate::{
    define_syscall, polling::poller::PollerObject, process::manager::get_current_process,
    s_println,
};

const DEADLOCK_LOG: bool = false;
const EPOLL_CTL_ADD: u64 = 1;
const EPOLL_CTL_DEL: u64 = 2;
const EPOLL_CTL_MOD: u64 = 3;
const EPOLLIN: u32 = 0x001;
const EPOLLOUT: u32 = 0x004;
const EPOLLERR: u32 = 0x008;
const EPOLLHUP: u32 = 0x010;

#[repr(C)]
union LinuxEpollData {
    ptr: u64,
    fd: i32,
    u32_: u32,
    u64_: u64,
}

#[repr(C)]
struct LinuxEpollEvent {
    events: u32,
    data: LinuxEpollData,
}

#[repr(C)]
pub struct PollResult {
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

fn linux_bits_to_events(bits: u32) -> [Option<PollableEvent>; 4] {
    [
        (bits & EPOLLIN != 0).then_some(PollableEvent::CanBeRead),
        (bits & EPOLLOUT != 0).then_some(PollableEvent::CanBeWritten),
        (bits & EPOLLERR != 0).then_some(PollableEvent::Error),
        (bits & EPOLLHUP != 0).then_some(PollableEvent::Closed),
    ]
}

define_syscall!(EpollCreate1, {
    let process = get_current_process();
    let objects = &mut process.lock().objects;

    objects.push(Some(Arc::new(PollerObject::new())));

    Ok(objects.len() - 1)
});

fn epoll_update_impl(
    poller: ObjectRef,
    target_object: ObjectRef,
    bits: u32,
    data: u64,
) -> Result<usize, SyscallError> {
    if target_object.clone().as_pollable().is_err() {
        if DEADLOCK_LOG {
            s_println!(
                "poller_add reject: bits=0x{:x} data={:#x} target not pollable",
                bits,
                data
            );
        }
        return Err(SyscallError::PermissionDenied);
    }

    for event in linux_bits_to_events(bits).into_iter().flatten() {
        poller
            .clone()
            .as_poller()?
            .register_obj(target_object.clone(), event, data);
    }

    Ok(0)
}

define_syscall!(EpollCtl, |poller: ObjectRef, op: u64, target_object: ObjectRef, event: u64| {
    match op {
        EPOLL_CTL_ADD | EPOLL_CTL_MOD => {
            let event = event as *const LinuxEpollEvent;
            if event.is_null() {
                return Err(SyscallError::BadAddress);
            }
            let event = unsafe { &*event };
            for existing in [PollableEvent::CanBeRead, PollableEvent::CanBeWritten, PollableEvent::Error, PollableEvent::Closed] {
                poller
                    .clone()
                    .as_poller()?
                    .unregister_obj(target_object.clone(), existing);
            }
            epoll_update_impl(poller, target_object, event.events, unsafe { event.data.u64_ })
        }
        EPOLL_CTL_DEL => {
            for existing in [PollableEvent::CanBeRead, PollableEvent::CanBeWritten, PollableEvent::Error, PollableEvent::Closed] {
                poller
                    .clone()
                    .as_poller()?
                    .unregister_obj(target_object.clone(), existing);
            }
            Ok(0)
        }
        _ => Err(SyscallError::InvalidArguments),
    }
});

fn epoll_wait_impl(
    poller: ObjectRef,
    events_ptr: *mut PollResult,
    maxevents: usize,
    timeout: i32,
) -> Result<usize, SyscallError> {
    if maxevents == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let poller = poller.as_poller()?;

    if !poller.has_woken_events() {
        poller.push_already_ready_events();
    }

    if !poller.has_woken_events() {
        if timeout == 0 {
            return Ok(0);
        }

        let deadline = if timeout < 0 {
            None
        } else {
            Some(Time::since_boot().add_ms(timeout as u64))
        };

        let poller_ref: Arc<dyn crate::object::Object> = poller.clone();
        let current = prepare_block_current(BlockType::WakeRequired {
            wake_type: WakeType::Poller(poller_ref),
            deadline,
        });

        if !poller.has_woken_events() {
            poller.push_already_ready_events();
        }

        if poller.has_woken_events() {
            cancel_block(&current);
        } else {
            finish_block_current();
        }
    }

    let woken_events = poller.take_woken_events(maxevents);
    if DEADLOCK_LOG && !woken_events.is_empty() {
        s_println!("poller_wait: woke {} event(s)", woken_events.len());
    }

    if !events_ptr.is_null() {
        for (index, woken) in woken_events.iter().enumerate() {
            unsafe {
                events_ptr.add(index).write(PollResult {
                    events: pollable_event_to_linux_bits(woken.event),
                    _pad: 0,
                    data: woken.data,
                });
            }
        }
    }

    Ok(woken_events.len())
}

define_syscall!(EpollWait, |poller: ObjectRef,
                           events_ptr: *mut PollResult,
                           maxevents: usize,
                           timeout: i32| {
    epoll_wait_impl(poller, events_ptr, maxevents, timeout)
});

define_syscall!(EpollPwait, |poller: ObjectRef,
                            events_ptr: *mut PollResult,
                            maxevents: usize,
                            timeout: i32| {
    epoll_wait_impl(poller, events_ptr, maxevents, timeout)
});
