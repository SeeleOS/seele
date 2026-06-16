use alloc::{sync::Arc, vec, vec::Vec};
use core::mem::size_of;

use crate::object::Object;
use crate::object::misc::get_object_current_process;
use crate::polling::event::PollableEvent;
use crate::polling::poller::PollerObject;
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::thread::yielding::{cancel_block, finish_block_current, prepare_block_current};
use crate::{
    define_syscall,
    filesystem::object::poll_identity_object,
    memory::user_safe,
    misc::time::Time,
    signal::{Signal, Signals},
    thread::{get_current_thread, yielding::BlockType, yielding::WakeType},
};

#[repr(C)]
#[derive(Clone, Copy)]
pub(in crate::systemcall) struct Timespec {
    pub(in crate::systemcall) tv_sec: i64,
    pub(in crate::systemcall) tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SigSetWithSize {
    sigmask: *const u64,
    sigsetsize: usize,
}

fn has_unblocked_pending_signals() -> bool {
    let current = get_current_thread();
    let (blocked_signals, pending_signals, parent) = {
        let current = current.lock();
        (
            current.blocked_signals,
            current.pending_signals,
            current.parent.clone(),
        )
    };
    let pending_signals = pending_signals | parent.lock().pending_signals;
    let unblockable = Signals::from(Signal::SIGKILL) | Signals::from(Signal::SIGSTOP);
    let deliverable = (pending_signals - blocked_signals) | (pending_signals & unblockable);
    !deliverable.is_empty()
}

fn block_on_poller(poller: Arc<PollerObject>, timeout: Option<Time>) -> Result<(), SyscallError> {
    if poller.has_woken_events() || poller.push_already_ready_events() {
        return Ok(());
    }

    if has_unblocked_pending_signals() {
        return Err(SyscallError::Interrupted);
    }

    let poller_ref: Arc<dyn Object> = poller.clone();
    let current = prepare_block_current(BlockType::WakeRequired {
        wake_type: WakeType::Poller(poller_ref),
        deadline: timeout,
    });

    finish_block_current();
    cancel_block(&current);

    if has_unblocked_pending_signals() {
        return Err(SyscallError::Interrupted);
    }

    Ok(())
}

fn sleep_without_fds(timeout: Option<Time>) -> Result<(), SyscallError> {
    if has_unblocked_pending_signals() {
        return Err(SyscallError::Interrupted);
    }

    let current = match timeout {
        Some(deadline) => prepare_block_current(BlockType::SetTime(deadline)),
        None => prepare_block_current(BlockType::WakeRequired {
            wake_type: WakeType::IO,
            deadline: None,
        }),
    };

    finish_block_current();
    cancel_block(&current);

    if has_unblocked_pending_signals() {
        return Err(SyscallError::Interrupted);
    }

    Ok(())
}

pub(in crate::systemcall) fn fdset_words(nfds: usize) -> usize {
    nfds.div_ceil(64)
}

#[cfg(test)]
pub(in crate::systemcall) unsafe fn fdset_contains(fdset: *const u64, fd: usize) -> bool {
    let word = fd / 64;
    let bit = fd % 64;
    // SAFETY: caller guarantees fdset is valid for the requested nfds.
    (unsafe { *fdset.add(word) } & (1u64 << bit)) != 0
}

#[cfg(test)]
pub(in crate::systemcall) unsafe fn fdset_insert(fdset: *mut u64, fd: usize) {
    let word = fd / 64;
    let bit = fd % 64;
    // SAFETY: caller guarantees fdset is valid for the requested nfds.
    unsafe { *fdset.add(word) |= 1u64 << bit };
}

#[cfg(test)]
pub(in crate::systemcall) unsafe fn clear_fdset(fdset: *mut u64, nfds: usize) {
    for index in 0..fdset_words(nfds) {
        // SAFETY: caller guarantees fdset is valid for the requested nfds.
        unsafe { *fdset.add(index) = 0 };
    }
}

#[cfg(test)]
pub(in crate::systemcall) fn timeout_to_deadline(
    timeout: *const Timespec,
) -> Result<Option<Time>, SyscallError> {
    if timeout.is_null() {
        return Ok(None);
    }

    let timeout = unsafe { &*timeout };
    if timeout.tv_sec < 0 || timeout.tv_nsec < 0 || timeout.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::InvalidArguments);
    }

    let timeout_ns = (timeout.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(timeout.tv_nsec as u64);
    Ok(Some(Time::since_boot().add_ns(timeout_ns)))
}

#[cfg(test)]
pub(in crate::systemcall) fn timeout_is_zero(timeout: *const Timespec) -> bool {
    if timeout.is_null() {
        return false;
    }

    let timeout = unsafe { &*timeout };
    timeout.tv_sec == 0 && timeout.tv_nsec == 0
}

fn timeout_to_deadline_value(timeout: Timespec) -> Result<Time, SyscallError> {
    if timeout.tv_sec < 0 || timeout.tv_nsec < 0 || timeout.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::InvalidArguments);
    }

    let timeout_ns = (timeout.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(timeout.tv_nsec as u64);
    Ok(Time::since_boot().add_ns(timeout_ns))
}

fn read_fdset(fdset: *const u64, nfds: usize) -> Result<Option<Vec<u64>>, SyscallError> {
    if fdset.is_null() {
        return Ok(None);
    }

    let mut words = Vec::with_capacity(fdset_words(nfds));
    for index in 0..fdset_words(nfds) {
        words.push(user_safe::read(unsafe { fdset.add(index) })?);
    }
    Ok(Some(words))
}

fn fdset_slice_contains(fdset: &[u64], fd: usize) -> bool {
    let word = fd / 64;
    let bit = fd % 64;
    fdset
        .get(word)
        .is_some_and(|entry| (*entry & (1u64 << bit)) != 0)
}

fn register_interest(
    poller: &Arc<PollerObject>,
    fdset: Option<&[u64]>,
    nfds: usize,
    watched: PollableEvent,
    event_ready: &mut [bool],
    ready_fds: &mut [bool],
    ready_count: &mut usize,
) -> Result<(), SyscallError> {
    let Some(fdset) = fdset else {
        return Ok(());
    };

    for fd in 0..nfds {
        let watched_fd = fdset_slice_contains(fdset, fd);
        if !watched_fd {
            continue;
        }

        let object = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
        let poll_object = poll_identity_object(object);
        if let Ok(pollable) = poll_object.clone().as_pollable() {
            if pollable.is_event_ready(watched) {
                event_ready[fd] = true;
                if !ready_fds[fd] {
                    ready_fds[fd] = true;
                    *ready_count += 1;
                }
            }
            poller.register_obj(poll_object, watched, fd as u64);
        } else {
            // Match relibc: non-epoll-capable descriptors should make select
            // return immediately rather than block forever.
            event_ready[fd] = true;
            if !ready_fds[fd] {
                ready_fds[fd] = true;
                *ready_count += 1;
            }
        }
    }

    Ok(())
}

fn rewrite_fdset(fdset: *mut u64, ready: &[bool], nfds: usize) -> Result<(), SyscallError> {
    if fdset.is_null() {
        return Ok(());
    }

    let words = fdset_words(nfds);
    for word_index in 0..words {
        let mut word = 0u64;
        for bit in 0..64 {
            let fd = word_index * 64 + bit;
            if fd >= nfds {
                break;
            }
            if ready[fd] {
                word |= 1u64 << bit;
            }
        }
        user_safe::write(unsafe { fdset.add(word_index) }, &word)?;
    }

    Ok(())
}

fn collect_ready(
    poller: &Arc<PollerObject>,
    nfds: usize,
    read_ready: &mut [bool],
    write_ready: &mut [bool],
    except_ready: &mut [bool],
    ready_fds: &mut [bool],
    ready_count: &mut usize,
) {
    for ready in poller.take_woken_events(nfds) {
        let fd = ready.data as usize;
        if fd >= nfds {
            continue;
        }

        match ready.event {
            PollableEvent::CanBeRead | PollableEvent::Closed | PollableEvent::ReadClosed => {
                read_ready[fd] = true;
            }
            PollableEvent::CanBeWritten => write_ready[fd] = true,
            PollableEvent::Error => except_ready[fd] = true,
            PollableEvent::Other(_) => {}
        }

        if !ready_fds[fd] {
            ready_fds[fd] = true;
            *ready_count += 1;
        }
    }
}

define_syscall!(
    Pselect6,
    |nfds: i32,
     readfds: *mut u64,
     writefds: *mut u64,
     exceptfds: *mut u64,
     timeout: *const Timespec,
     sigmask: *const SigSetWithSize| {
        if nfds < 0 {
            return Err(SyscallError::InvalidArguments);
        }

        let requested_sigmask = if sigmask.is_null() {
            None
        } else {
            let sigmask = user_safe::read(sigmask)?;
            if !sigmask.sigmask.is_null() && sigmask.sigsetsize != size_of::<u64>() {
                return Err(SyscallError::InvalidArguments);
            }
            if sigmask.sigmask.is_null() {
                None
            } else {
                Some(Signals::from_bits_truncate(user_safe::read(
                    sigmask.sigmask,
                )?))
            }
        };

        let nfds = nfds as usize;
        let timeout = if timeout.is_null() {
            None
        } else {
            Some(user_safe::read(timeout)?)
        };
        let readfds_in = read_fdset(readfds.cast_const(), nfds)?;
        let writefds_in = read_fdset(writefds.cast_const(), nfds)?;
        let exceptfds_in = read_fdset(exceptfds.cast_const(), nfds)?;

        let old_mask = requested_sigmask.map(|new_mask| {
            let current = get_current_thread();
            let mut current = current.lock();
            let old_mask = current.blocked_signals;
            current.blocked_signals = new_mask;
            old_mask
        });

        let result = (|| {
            let timeout_is_zero = timeout
                .as_ref()
                .is_some_and(|timeout| timeout.tv_sec == 0 && timeout.tv_nsec == 0);
            if nfds == 0 {
                if !timeout_is_zero {
                    let deadline = timeout.map(timeout_to_deadline_value).transpose()?;
                    sleep_without_fds(deadline)?;
                }
                return Ok(0);
            }

            let poller = PollerObject::new();
            let mut ready_fds = vec![false; nfds];
            let mut read_ready = vec![false; nfds];
            let mut write_ready = vec![false; nfds];
            let mut except_ready = vec![false; nfds];
            let mut ready_count = 0usize;

            register_interest(
                &poller,
                readfds_in.as_deref(),
                nfds,
                PollableEvent::CanBeRead,
                &mut read_ready,
                &mut ready_fds,
                &mut ready_count,
            )?;
            register_interest(
                &poller,
                readfds_in.as_deref(),
                nfds,
                PollableEvent::Closed,
                &mut read_ready,
                &mut ready_fds,
                &mut ready_count,
            )?;
            register_interest(
                &poller,
                readfds_in.as_deref(),
                nfds,
                PollableEvent::ReadClosed,
                &mut read_ready,
                &mut ready_fds,
                &mut ready_count,
            )?;
            register_interest(
                &poller,
                writefds_in.as_deref(),
                nfds,
                PollableEvent::CanBeWritten,
                &mut write_ready,
                &mut ready_fds,
                &mut ready_count,
            )?;
            register_interest(
                &poller,
                writefds_in.as_deref(),
                nfds,
                PollableEvent::Closed,
                &mut write_ready,
                &mut ready_fds,
                &mut ready_count,
            )?;
            register_interest(
                &poller,
                exceptfds_in.as_deref(),
                nfds,
                PollableEvent::Error,
                &mut except_ready,
                &mut ready_fds,
                &mut ready_count,
            )?;

            if ready_count == 0 && !timeout_is_zero {
                let deadline = timeout.map(timeout_to_deadline_value).transpose()?;
                block_on_poller(poller.clone(), deadline)?;
            }

            collect_ready(
                &poller,
                nfds,
                &mut read_ready,
                &mut write_ready,
                &mut except_ready,
                &mut ready_fds,
                &mut ready_count,
            );

            rewrite_fdset(readfds, &read_ready, nfds)?;
            rewrite_fdset(writefds, &write_ready, nfds)?;
            rewrite_fdset(exceptfds, &except_ready, nfds)?;

            Ok(ready_count)
        })();

        if let Some(old_mask) = old_mask {
            get_current_thread().lock().blocked_signals = old_mask;
        }

        result
    }
);

#[cfg(test)]
mod tests {
    use crate::systemcall::test::*;

    crate::test!(
        select_fdset_helpers,
        "select fdset helpers count clear test and set words",
        select_fdset_helpers_count_clear_test_and_set_words
    );
    crate::test!(
        select_timeout_validation,
        "select timeout helpers validate null zero and invalid timespecs",
        select_timeout_helpers_validate_null_zero_and_invalid_timespecs
    );
    crate::test!(
        pselect6_syscalls,
        "pselect6 follows linux rules",
        pselect6_syscalls_follow_linux_rules
    );
}
