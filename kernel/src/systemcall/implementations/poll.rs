use alloc::{sync::Arc, vec, vec::Vec};
use bitflags::bitflags;

use crate::{
    define_syscall,
    filesystem::object::poll_identity_object,
    memory::user_safe,
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
    pub(crate) struct PollEvents: i16 {
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
        const POLLRDHUP = 0x2000;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxPollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(in crate::systemcall) struct Timespec {
    pub(in crate::systemcall) tv_sec: i64,
    pub(in crate::systemcall) tv_nsec: i64,
}

pub(in crate::systemcall) fn kernel_events_for(bits: PollEvents) -> [Option<PollableEvent>; 5] {
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
        (watch_any || bits.contains(PollEvents::POLLRDHUP)).then_some(PollableEvent::ReadClosed),
    ]
}

pub(in crate::systemcall) fn translate_ready_events(
    requested_events: PollEvents,
    kernel_events: u32,
) -> i16 {
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
    if kernel_events & (PollEvents::POLLRDHUP.bits() as u32) != 0 {
        translated |= requested_events
            & (PollEvents::POLLIN
                | PollEvents::POLLPRI
                | PollEvents::POLLRDNORM
                | PollEvents::POLLRDBAND);
        translated |= requested_events & PollEvents::POLLRDHUP;
    }

    translated.bits()
}

fn count_ready(fds: &[LinuxPollFd]) -> usize {
    fds.iter().filter(|pfd| pfd.revents != 0).count()
}

fn read_pollfds(fds: *const LinuxPollFd, nfds: usize) -> Result<Vec<LinuxPollFd>, SyscallError> {
    let mut local = Vec::with_capacity(nfds);
    for index in 0..nfds {
        local.push(user_safe::read(unsafe { fds.add(index) })?);
    }
    Ok(local)
}

fn write_pollfds_revents(fds: *mut LinuxPollFd, local: &[LinuxPollFd]) -> Result<(), SyscallError> {
    for (index, pollfd) in local.iter().enumerate() {
        user_safe::write(unsafe { fds.add(index) }, pollfd)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        signal::Signal,
        systemcall::{
            implementations::{Eventfd, Poll, Ppoll},
            test::{TestLinuxPollFd, TestLinuxTimespec, close_test_fd, expect_fd},
            test_helpers::{
                SyscallArgs, allocate_user_test_page, assert_linux_layout, expect_errno, expect_ok,
                read_user_value, write_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        poll_and_ppoll_syscalls,
        "poll and ppoll follow linux rules",
        poll_and_ppoll_syscalls_follow_linux_rules
    );
    crate::test!(
        poll_event_translation,
        "poll helpers translate linux events to kernel readiness",
        poll_helpers_translate_linux_events_to_kernel_readiness
    );
    crate::test!(
        poll_timeout_validation,
        "poll timeout helpers reject invalid timespecs and saturate",
        poll_timeout_helpers_reject_invalid_timespecs_and_saturate
    );

    fn poll_and_ppoll_syscalls_follow_linux_rules() {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        const POLLNVAL: i16 = 0x020;

        assert_linux_layout::<TestLinuxPollFd>(8, 4);

        let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let poll_page = allocate_user_test_page();
        write_user_value(
            poll_page,
            &[
                TestLinuxPollFd {
                    fd: eventfd as i32,
                    events: POLLOUT,
                    revents: 0,
                },
                TestLinuxPollFd {
                    fd: 4096,
                    events: POLLIN,
                    revents: 0,
                },
            ],
        );
        expect_ok(
            SyscallArgs::new([poll_page, 2, 0, 0, 0, 0]).call::<Poll>(),
            2,
        );
        let pollfds = read_user_value::<[TestLinuxPollFd; 2]>(poll_page);
        assert_eq!(pollfds[0].revents & POLLOUT, POLLOUT);
        assert_eq!(pollfds[1].revents & POLLNVAL, POLLNVAL);

        write_user_value(
            poll_page,
            &[TestLinuxPollFd {
                fd: eventfd as i32,
                events: POLLIN,
                revents: 123,
            }],
        );
        expect_ok(
            SyscallArgs::new([poll_page, 1, 0, 0, 0, 0]).call::<Poll>(),
            0,
        );
        assert_eq!(read_user_value::<TestLinuxPollFd>(poll_page).revents, 0);

        let ppoll_timeout = TestLinuxTimespec {
            tv_sec: 0,
            tv_nsec: 1_000_000,
        };
        write_user_value(
            poll_page,
            &[TestLinuxPollFd {
                fd: eventfd as i32,
                events: POLLOUT,
                revents: 0,
            }],
        );
        write_user_value(poll_page + 128, &ppoll_timeout);
        let ppoll_result =
            SyscallArgs::new([poll_page, 1, poll_page + 128, 0, 0, 0]).call::<Ppoll>();
        expect_ok(ppoll_result, 1);
        assert_eq!(
            read_user_value::<TestLinuxPollFd>(poll_page).revents & POLLOUT,
            POLLOUT
        );
        let sigmask: u64 = Signal::SIGUSR1.mask();
        write_user_value(poll_page + 192, &sigmask);
        expect_errno(
            SyscallArgs::new([poll_page, 1, poll_page + 128, poll_page + 192, 4, 0])
                .call::<Ppoll>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            poll_page + 128,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
        );
        expect_errno(
            SyscallArgs::new([poll_page, 1, poll_page + 128, 0, 0, 0]).call::<Ppoll>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Poll>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Ppoll>(),
            SyscallError::BadAddress,
        );

        close_test_fd(eventfd);
    }

    fn poll_helpers_translate_linux_events_to_kernel_readiness() {
        let events = kernel_events_for(PollEvents::POLLIN | PollEvents::POLLOUT);

        assert_eq!(events[0], Some(PollableEvent::CanBeRead));
        assert_eq!(events[1], Some(PollableEvent::CanBeWritten));
        assert_eq!(events[2], Some(PollableEvent::Error));
        assert_eq!(events[3], Some(PollableEvent::Closed));
        assert_eq!(events[4], Some(PollableEvent::ReadClosed));

        let translated = translate_ready_events(
            PollEvents::POLLIN
                | PollEvents::POLLRDNORM
                | PollEvents::POLLHUP
                | PollEvents::POLLRDHUP,
            (PollEvents::POLLIN | PollEvents::POLLHUP | PollEvents::POLLRDHUP).bits() as u32,
        );
        let translated = PollEvents::from_bits_retain(translated);
        assert!(translated.contains(PollEvents::POLLIN));
        assert!(translated.contains(PollEvents::POLLRDNORM));
        assert!(translated.contains(PollEvents::POLLHUP));
        assert!(translated.contains(PollEvents::POLLRDHUP));
        assert!(!translated.contains(PollEvents::POLLOUT));
    }

    fn poll_timeout_helpers_reject_invalid_timespecs_and_saturate() {
        assert_eq!(
            saturating_timeout_ms(&Timespec {
                tv_sec: 1,
                tv_nsec: 999_999_999,
            })
            .unwrap(),
            1999
        );
        assert_eq!(
            saturating_timeout_ms(&Timespec {
                tv_sec: i64::MAX,
                tv_nsec: 0,
            })
            .unwrap(),
            i32::MAX
        );
        assert!(matches!(
            saturating_timeout_ms(&Timespec {
                tv_sec: -1,
                tv_nsec: 0,
            }),
            Err(SyscallError::InvalidArguments)
        ));
        assert!(matches!(
            saturating_timeout_ms(&Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            }),
            Err(SyscallError::InvalidArguments)
        ));
    }
}

pub(in crate::systemcall) fn saturating_timeout_ms(
    timeout: &Timespec,
) -> Result<i32, SyscallError> {
    if timeout.tv_sec < 0 || timeout.tv_nsec < 0 || timeout.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::InvalidArguments);
    }

    if timeout.tv_sec > (i32::MAX as i64 / 1000) {
        return Ok(i32::MAX);
    }

    Ok((timeout.tv_sec as i32) * 1000 + (timeout.tv_nsec as i32) / 1_000_000)
}

fn wait_on_poller(poller: Arc<PollerObject>, timeout_ms: i32) -> Result<(), SyscallError> {
    if !poller.has_woken_events() {
        poller.push_already_ready_events();
    }

    if poller.has_woken_events() {
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

    finish_block_current();
    cancel_block(&current);

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
            pfd.revents |= ((PollEvents::from_bits_retain(pfd.events))
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

        let requested_events = PollEvents::from_bits_retain(pfd.events);
        for event in kernel_events_for(requested_events).into_iter().flatten() {
            poller.register_obj(poll_object.clone(), event, index as u64);
        }
    }

    if invalid > 0 && invalid == active {
        return Ok(count_ready(fds));
    }

    let already_ready = poller.push_already_ready_events();
    if count_ready(fds) == 0 && !already_ready {
        wait_on_poller(poller.clone(), timeout_ms)?;
    }

    let mut ready_by_index = vec![0; fds.len()];
    for ready in poller.take_woken_events(fds.len()) {
        let index = ready.data as usize;
        if let Some(events) = ready_by_index.get_mut(index) {
            *events |= ready.ready_bits;
        }
    }

    for (index, kernel_ready) in ready_by_index.into_iter().enumerate() {
        if kernel_ready != 0 {
            let pfd = &mut fds[index];
            pfd.revents |=
                translate_ready_events(PollEvents::from_bits_retain(pfd.events), kernel_ready);
        }
    }

    Ok(count_ready(fds))
}

define_syscall!(Poll, |fds: *mut LinuxPollFd, nfds: usize, timeout: i32| {
    if fds.is_null() && nfds != 0 {
        return Err(SyscallError::BadAddress);
    }

    let mut local_fds = read_pollfds(fds, nfds)?;
    let result = poll_impl(&mut local_fds, timeout)?;
    write_pollfds_revents(fds, &local_fds)?;
    Ok(result)
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
        let timeout = &user_safe::read(timeout)?;
        saturating_timeout_ms(timeout)?
    };

    if fds.is_null() && nfds != 0 {
        return Err(SyscallError::BadAddress);
    }

    let mut local_fds = read_pollfds(fds, nfds)?;
    let result = poll_impl(&mut local_fds, timeout_ms)?;
    write_pollfds_revents(fds, &local_fds)?;
    Ok(result)
});
