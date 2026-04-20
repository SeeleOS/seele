use alloc::{collections::BTreeMap, sync::Arc};
use bitflags::bitflags;

use crate::{
    define_syscall,
    filesystem::object::poll_identity_object,
    misc::{error::AsSyscallError, time::Time},
    object::{Object, error::ObjectError, misc::get_object_current_process},
    polling::{event::PollableEvent, poller::PollerObject},
    systemcall::utils::{SyscallError, SyscallImpl},
    thread::yielding::{
        BlockType, WakeType, block_current_with_sig_check, cancel_block, finish_block_current,
        prepare_block_current,
    },
};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct PollEvents: i16 {
        const POLLIN = 0x001;
        const POLLPRI = 0x002;
        const POLLOUT = 0x004;
        const POLLERR = 0x008;
        const POLLHUP = 0x010;
        const POLLNVAL = 0x020;
        const POLLRDNORM = 0x040;
        const POLLRDBAND = 0x080;
        const POLLWRNORM = 0x100;
        const POLLWRBAND = 0x200;
    }
}

#[repr(C)]
struct LinuxPollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn kernel_events_for(bits: i16) -> [Option<PollableEvent>; 4] {
    let bits = PollEvents::from_bits_retain(bits);
    let watch_read = bits.intersects(
        PollEvents::POLLIN | PollEvents::POLLPRI | PollEvents::POLLRDNORM | PollEvents::POLLRDBAND,
    );
    let watch_write =
        bits.intersects(PollEvents::POLLOUT | PollEvents::POLLWRNORM | PollEvents::POLLWRBAND);
    let watch_any = watch_read || watch_write;

    [
        watch_read.then_some(PollableEvent::CanBeRead),
        watch_write.then_some(PollableEvent::CanBeWritten),
        (watch_any || bits.contains(PollEvents::POLLERR)).then_some(PollableEvent::Error),
        (watch_any || bits.contains(PollEvents::POLLHUP)).then_some(PollableEvent::Closed),
    ]
}

fn translate_ready_events(requested_events: i16, kernel_events: u32) -> i16 {
    let requested_events = PollEvents::from_bits_retain(requested_events);
    let mut translated = PollEvents::empty();

    if kernel_events & (PollEvents::POLLIN.bits() as u32) != 0 {
        translated |= requested_events
            & (PollEvents::POLLIN
                | PollEvents::POLLPRI
                | PollEvents::POLLRDNORM
                | PollEvents::POLLRDBAND);
    }
    if kernel_events & (PollEvents::POLLOUT.bits() as u32) != 0 {
        translated |= requested_events
            & (PollEvents::POLLOUT | PollEvents::POLLWRNORM | PollEvents::POLLWRBAND);
    }
    if kernel_events & (PollEvents::POLLERR.bits() as u32) != 0 {
        translated |= PollEvents::POLLERR;
    }
    if kernel_events & (PollEvents::POLLHUP.bits() as u32) != 0 {
        translated |= PollEvents::POLLHUP;
    }

    translated.bits()
}

fn count_ready(fds: &[LinuxPollFd]) -> usize {
    fds.iter().filter(|pfd| pfd.revents != 0).count()
}

fn saturating_timeout_ms(timeout: &Timespec) -> Result<i32, SyscallError> {
    if timeout.tv_sec < 0 || timeout.tv_nsec < 0 || timeout.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::InvalidArguments);
    }

    if timeout.tv_sec > (i32::MAX as i64 / 1000) {
        return Ok(i32::MAX);
    }

    Ok((timeout.tv_sec as i32) * 1000 + (timeout.tv_nsec as i32) / 1_000_000)
}

fn wait_on_poller(poller: Arc<PollerObject>, timeout_ms: i32) -> Result<(), SyscallError> {
    if poller.has_woken_events() || poller.push_already_ready_events() {
        return Ok(());
    }

    if timeout_ms == 0 {
        return Ok(());
    }

    let deadline = if timeout_ms < 0 {
        None
    } else {
        Some(Time::since_boot().add_ms(timeout_ms as u64))
    };

    let poller_ref: Arc<dyn Object> = poller.clone();
    let current = prepare_block_current(BlockType::WakeRequired {
        wake_type: WakeType::Poller(poller_ref),
        deadline,
    });

    if !poller.has_woken_events() {
        poller.push_already_ready_events();
    }

    if poller.has_woken_events() {
        cancel_block(&current);
        return Ok(());
    }

    finish_block_current();
    Ok(())
}

fn sleep_without_fds(timeout_ms: i32) -> Result<(), SyscallError> {
    if timeout_ms == 0 {
        return Ok(());
    }

    if timeout_ms < 0 {
        loop {
            block_current_with_sig_check(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            })
            .map_err(|err| err.as_syscall_error())?;
        }
    }

    block_current_with_sig_check(BlockType::SetTime(
        Time::since_boot().add_ms(timeout_ms as u64),
    ))
    .map_err(|err| err.as_syscall_error())
}

fn poll_impl(fds: &mut [LinuxPollFd], timeout_ms: i32) -> Result<usize, SyscallError> {
    for pfd in fds.iter_mut() {
        pfd.revents = 0;
    }

    if fds.is_empty() {
        sleep_without_fds(timeout_ms)?;
        return Ok(0);
    }

    let poller = PollerObject::new();
    let mut active = 0usize;
    let mut invalid = 0usize;

    for (index, pfd) in fds.iter_mut().enumerate() {
        if pfd.fd < 0 {
            continue;
        }
        active += 1;

        let object = match get_object_current_process(pfd.fd as u64) {
            Ok(object) => object,
            Err(err) => {
                if matches!(err, ObjectError::DoesNotExist) {
                    pfd.revents |= PollEvents::POLLNVAL.bits();
                    invalid += 1;
                    continue;
                }
                return Err(err.as_syscall_error());
            }
        };

        let poll_object = poll_identity_object(object.clone());

        if poll_object.clone().as_pollable().is_err() {
            pfd.revents |= (PollEvents::from_bits_retain(pfd.events)
                & (PollEvents::POLLIN
                    | PollEvents::POLLPRI
                    | PollEvents::POLLRDNORM
                    | PollEvents::POLLRDBAND
                    | PollEvents::POLLOUT
                    | PollEvents::POLLWRNORM
                    | PollEvents::POLLWRBAND))
                .bits();
            continue;
        }

        for event in kernel_events_for(pfd.events).into_iter().flatten() {
            poller.register_obj(poll_object.clone(), event, index as u64);
        }
    }

    if invalid > 0 && invalid == active {
        return Ok(count_ready(fds));
    }

    if count_ready(fds) == 0 {
        wait_on_poller(poller.clone(), timeout_ms)?;
    }

    let mut ready_by_index = BTreeMap::<usize, u32>::new();
    for ready in poller.take_woken_events(fds.len()) {
        ready_by_index
            .entry(ready.data as usize)
            .and_modify(|events| {
                *events |= match ready.event {
                    PollableEvent::CanBeRead => PollEvents::POLLIN.bits() as u32,
                    PollableEvent::CanBeWritten => PollEvents::POLLOUT.bits() as u32,
                    PollableEvent::Error => PollEvents::POLLERR.bits() as u32,
                    PollableEvent::Closed => PollEvents::POLLHUP.bits() as u32,
                    PollableEvent::Other(bits) => bits as u32,
                }
            })
            .or_insert_with(|| match ready.event {
                PollableEvent::CanBeRead => PollEvents::POLLIN.bits() as u32,
                PollableEvent::CanBeWritten => PollEvents::POLLOUT.bits() as u32,
                PollableEvent::Error => PollEvents::POLLERR.bits() as u32,
                PollableEvent::Closed => PollEvents::POLLHUP.bits() as u32,
                PollableEvent::Other(bits) => bits as u32,
            });
    }

    for (index, kernel_ready) in ready_by_index {
        if let Some(pfd) = fds.get_mut(index) {
            pfd.revents |= translate_ready_events(pfd.events, kernel_ready);
        }
    }

    Ok(count_ready(fds))
}

define_syscall!(Poll, |fds: *mut LinuxPollFd, nfds: usize, timeout: i32| {
    if fds.is_null() && nfds != 0 {
        return Err(SyscallError::BadAddress);
    }

    let fds = unsafe { core::slice::from_raw_parts_mut(fds, nfds) };
    poll_impl(fds, timeout)
});

define_syscall!(Ppoll, |fds: *mut LinuxPollFd,
                        nfds: usize,
                        timeout: *const Timespec,
                        sigmask: *const u64,
                        sigsetsize: usize| {
    if !sigmask.is_null() && sigsetsize != core::mem::size_of::<u64>() {
        return Err(SyscallError::InvalidArguments);
    }

    let timeout_ms = if timeout.is_null() {
        -1
    } else {
        let timeout = unsafe { &*timeout };
        saturating_timeout_ms(timeout)?
    };

    if fds.is_null() && nfds != 0 {
        return Err(SyscallError::BadAddress);
    }

    let fds = unsafe { core::slice::from_raw_parts_mut(fds, nfds) };
    poll_impl(fds, timeout_ms)
});
