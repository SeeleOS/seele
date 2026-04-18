use alloc::{sync::Arc, vec};

use crate::define_syscall;
use crate::filesystem::object::poll_identity_object;
use crate::misc::time::Time;
use crate::object::misc::get_object_current_process;
use crate::polling::event::PollableEvent;
use crate::polling::poller::PollerObject;
use crate::systemcall::utils::{SyscallError, SyscallImpl};
use crate::thread::yielding::{
    BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
};

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
struct SigSetWithSize {
    sigmask: *const u64,
    sigsetsize: usize,
}

fn block_on_poller(poller: Arc<PollerObject>, timeout: Option<Time>) {
    if poller.has_woken_events() || poller.push_already_ready_events() {
        return;
    }

    let poller_ref: Arc<dyn crate::object::Object> = poller;
    let current = prepare_block_current(BlockType::WakeRequired {
        wake_type: WakeType::Poller(poller_ref),
        deadline: timeout,
    });

    finish_block_current();
    cancel_block(&current);
}

fn fdset_words(nfds: usize) -> usize {
    nfds.div_ceil(64)
}

unsafe fn fdset_contains(fdset: *const u64, fd: usize) -> bool {
    let word = fd / 64;
    let bit = fd % 64;
    // SAFETY: caller guarantees fdset is valid for the requested nfds.
    (unsafe { *fdset.add(word) } & (1u64 << bit)) != 0
}

unsafe fn fdset_insert(fdset: *mut u64, fd: usize) {
    let word = fd / 64;
    let bit = fd % 64;
    // SAFETY: caller guarantees fdset is valid for the requested nfds.
    unsafe { *fdset.add(word) |= 1u64 << bit };
}

unsafe fn clear_fdset(fdset: *mut u64, nfds: usize) {
    for index in 0..fdset_words(nfds) {
        // SAFETY: caller guarantees fdset is valid for the requested nfds.
        unsafe { *fdset.add(index) = 0 };
    }
}

fn timeout_to_deadline(timeout: *const Timespec) -> Result<Option<Time>, SyscallError> {
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

fn timeout_is_zero(timeout: *const Timespec) -> bool {
    if timeout.is_null() {
        return false;
    }

    let timeout = unsafe { &*timeout };
    timeout.tv_sec == 0 && timeout.tv_nsec == 0
}

fn register_interest(
    poller: &Arc<PollerObject>,
    fdset: *const u64,
    nfds: usize,
    watched: PollableEvent,
    event_ready: &mut [bool],
    ready_fds: &mut [bool],
    ready_count: &mut usize,
) -> Result<(), SyscallError> {
    if fdset.is_null() {
        return Ok(());
    }

    for fd in 0..nfds {
        let watched_fd = unsafe { fdset_contains(fdset, fd) };
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

fn rewrite_fdset(fdset: *mut u64, ready: &[bool], nfds: usize) {
    if fdset.is_null() {
        return;
    }

    unsafe { clear_fdset(fdset, nfds) };
    for (fd, is_ready) in ready.iter().copied().enumerate() {
        if is_ready {
            unsafe { fdset_insert(fdset, fd) };
        }
    }
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
            PollableEvent::CanBeRead | PollableEvent::Closed => read_ready[fd] = true,
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

        if !sigmask.is_null() {
            let sigmask = unsafe { &*sigmask };
            if !sigmask.sigmask.is_null() && sigmask.sigsetsize != core::mem::size_of::<u64>() {
                return Err(SyscallError::InvalidArguments);
            }
        }

        let nfds = nfds as usize;

        let poller = PollerObject::new();
        let mut ready_fds = vec![false; nfds];
        let mut read_ready = vec![false; nfds];
        let mut write_ready = vec![false; nfds];
        let mut except_ready = vec![false; nfds];
        let mut ready_count = 0usize;

        register_interest(
            &poller,
            readfds.cast_const(),
            nfds,
            PollableEvent::CanBeRead,
            &mut read_ready,
            &mut ready_fds,
            &mut ready_count,
        )?;
        register_interest(
            &poller,
            readfds.cast_const(),
            nfds,
            PollableEvent::Closed,
            &mut read_ready,
            &mut ready_fds,
            &mut ready_count,
        )?;
        register_interest(
            &poller,
            writefds.cast_const(),
            nfds,
            PollableEvent::CanBeWritten,
            &mut write_ready,
            &mut ready_fds,
            &mut ready_count,
        )?;
        register_interest(
            &poller,
            writefds.cast_const(),
            nfds,
            PollableEvent::Closed,
            &mut write_ready,
            &mut ready_fds,
            &mut ready_count,
        )?;
        register_interest(
            &poller,
            exceptfds.cast_const(),
            nfds,
            PollableEvent::Error,
            &mut except_ready,
            &mut ready_fds,
            &mut ready_count,
        )?;

        if ready_count == 0 && !timeout_is_zero(timeout) {
            let deadline = timeout_to_deadline(timeout)?;
            block_on_poller(poller.clone(), deadline);
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

        rewrite_fdset(readfds, &read_ready, nfds);
        rewrite_fdset(writefds, &write_ready, nfds);
        rewrite_fdset(exceptfds, &except_ready, nfds);

        Ok(ready_count)
    }
);
