use crate::filesystem::object::poll_identity_object;
use crate::misc::time::Time;
use crate::object::misc::ObjectRef;
use crate::polling::event::PollableEvent;
use crate::systemcall::utils::SyscallImpl;
use crate::thread::yielding::{
    BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
};
use alloc::sync::Arc;
use num_enum::TryFromPrimitive;

use crate::systemcall::utils::SyscallError;
use crate::{define_syscall, polling::poller::PollerObject, process::manager::get_current_process};

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum EpollCtlOp {
    Add = 1,
    Del = 2,
    Mod = 3,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct EpollEvents: u32 {
        const IN = 0x001;
        const OUT = 0x004;
        const ERR = 0x008;
        const HUP = 0x010;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
union LinuxEpollData {
    ptr: u64,
    fd: i32,
    u32_: u32,
    u64_: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct LinuxEpollEvent {
    events: u32,
    data: LinuxEpollData,
}

fn read_epoll_event(event_ptr: *const LinuxEpollEvent) -> LinuxEpollEvent {
    unsafe { event_ptr.read_unaligned() }
}

fn epoll_event_data_u64(event: &LinuxEpollEvent) -> u64 {
    unsafe { core::ptr::addr_of!(event.data.u64_).read_unaligned() }
}

fn write_epoll_event(event_ptr: *mut LinuxEpollEvent, events: u32, data: u64) {
    unsafe {
        event_ptr.write_unaligned(LinuxEpollEvent {
            events,
            data: LinuxEpollData { u64_: data },
        });
    }
}

fn pollable_event_to_linux_bits(event: PollableEvent) -> u32 {
    match event {
        PollableEvent::CanBeRead => EpollEvents::IN.bits(),
        PollableEvent::CanBeWritten => EpollEvents::OUT.bits(),
        PollableEvent::Error => EpollEvents::ERR.bits(),
        PollableEvent::Closed => EpollEvents::HUP.bits(),
        PollableEvent::Other(bits) => bits as u32,
    }
}

fn linux_bits_to_events(bits: u32) -> [Option<PollableEvent>; 4] {
    let bits = EpollEvents::from_bits_truncate(bits);
    [
        bits.contains(EpollEvents::IN)
            .then_some(PollableEvent::CanBeRead),
        bits.contains(EpollEvents::OUT)
            .then_some(PollableEvent::CanBeWritten),
        bits.contains(EpollEvents::ERR)
            .then_some(PollableEvent::Error),
        bits.contains(EpollEvents::HUP)
            .then_some(PollableEvent::Closed),
    ]
}

define_syscall!(EpollCreate1, {
    let process = get_current_process();
    let objects = &mut process.lock().objects;

    objects.push(Some(PollerObject::new()));

    Ok(objects.len() - 1)
});

fn epoll_update_impl(
    poller: ObjectRef,
    target_object: ObjectRef,
    bits: u32,
    data: u64,
) -> Result<usize, SyscallError> {
    let target_object = poll_identity_object(target_object);

    if target_object.clone().as_pollable().is_err() {
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

define_syscall!(
    EpollCtl,
    |poller: ObjectRef, op: u64, target_object: ObjectRef, event: *const LinuxEpollEvent| {
        let target_object = poll_identity_object(target_object);

        match EpollCtlOp::try_from(op).map_err(|_| SyscallError::InvalidArguments)? {
            EpollCtlOp::Add | EpollCtlOp::Mod => {
                if event.is_null() {
                    return Err(SyscallError::BadAddress);
                }
                let event = read_epoll_event(event);
                for existing in [
                    PollableEvent::CanBeRead,
                    PollableEvent::CanBeWritten,
                    PollableEvent::Error,
                    PollableEvent::Closed,
                ] {
                    poller
                        .clone()
                        .as_poller()?
                        .unregister_obj(target_object.clone(), existing);
                }
                epoll_update_impl(
                    poller,
                    target_object,
                    event.events,
                    epoll_event_data_u64(&event),
                )
            }
            EpollCtlOp::Del => {
                for existing in [
                    PollableEvent::CanBeRead,
                    PollableEvent::CanBeWritten,
                    PollableEvent::Error,
                    PollableEvent::Closed,
                ] {
                    poller
                        .clone()
                        .as_poller()?
                        .unregister_obj(target_object.clone(), existing);
                }
                Ok(0)
            }
        }
    }
);

fn epoll_wait_impl(
    poller: ObjectRef,
    events_ptr: *mut LinuxEpollEvent,
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

    if !events_ptr.is_null() {
        for (index, woken) in woken_events.iter().enumerate() {
            write_epoll_event(
                unsafe { events_ptr.add(index) },
                pollable_event_to_linux_bits(woken.event),
                woken.data,
            );
        }
    }

    Ok(woken_events.len())
}

define_syscall!(EpollWait, |poller: ObjectRef,
                            events_ptr: *mut LinuxEpollEvent,
                            maxevents: usize,
                            timeout: i32| {
    epoll_wait_impl(poller, events_ptr, maxevents, timeout)
});

define_syscall!(EpollPwait, |poller: ObjectRef,
                             events_ptr: *mut LinuxEpollEvent,
                             maxevents: usize,
                             timeout: i32| {
    epoll_wait_impl(poller, events_ptr, maxevents, timeout)
});
