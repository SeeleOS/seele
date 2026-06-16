use crate::filesystem::object::poll_identity_object;
use crate::memory::user_safe;
use crate::misc::time::Time;
use crate::object::{Object, misc::ObjectRef};
use crate::polling::event::PollableEvent;
use crate::systemcall::utils::SyscallImpl;
use crate::thread::yielding::{
    BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
};
use alloc::sync::Arc;
use bitflags::bitflags;
use num_enum::TryFromPrimitive;

use crate::systemcall::utils::SyscallError;
use crate::{
    define_syscall,
    polling::poller::PollerObject,
    process::{FdFlags, manager::get_current_process},
};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct EpollCreateFlags: i32 {
        const EPOLL_CLOEXEC = 0o2_000_000;
    }
}

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(u64)]
enum EpollCtlOp {
    Add = 1,
    Del = 2,
    Mod = 3,
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct EpollEvents: u32 {
        const IN = 0x001;
        const PRI = 0x002;
        const OUT = 0x004;
        const ERR = 0x008;
        const HUP = 0x010;
        const RDHUP = 0x2000;
        const ET = 0x8000_0000;
        const ONESHOT = 0x4000_0000;
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

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn read_epoll_event(event_ptr: *const LinuxEpollEvent) -> Result<LinuxEpollEvent, SyscallError> {
    user_safe::read(event_ptr)
}

fn epoll_event_data_u64(event: &LinuxEpollEvent) -> u64 {
    unsafe { core::ptr::addr_of!(event.data.u64_).read_unaligned() }
}

fn write_epoll_event(
    event_ptr: *mut LinuxEpollEvent,
    events: u32,
    data: u64,
) -> Result<(), SyscallError> {
    user_safe::write(
        event_ptr,
        &LinuxEpollEvent {
            events,
            data: LinuxEpollData { u64_: data },
        },
    )
}

fn epoll_interest_entries(bits: EpollEvents) -> [(bool, PollableEvent, u32); 6] {
    let watch_any = bits.intersects(EpollEvents::IN | EpollEvents::PRI | EpollEvents::OUT);

    [
        (
            bits.contains(EpollEvents::IN),
            PollableEvent::CanBeRead,
            EpollEvents::IN.bits(),
        ),
        (
            bits.contains(EpollEvents::PRI),
            PollableEvent::CanBeRead,
            EpollEvents::PRI.bits(),
        ),
        (
            bits.contains(EpollEvents::OUT),
            PollableEvent::CanBeWritten,
            EpollEvents::OUT.bits(),
        ),
        (
            watch_any || bits.contains(EpollEvents::ERR),
            PollableEvent::Error,
            EpollEvents::ERR.bits(),
        ),
        (
            watch_any || bits.contains(EpollEvents::HUP),
            PollableEvent::Closed,
            EpollEvents::HUP.bits(),
        ),
        (
            watch_any || bits.contains(EpollEvents::RDHUP),
            PollableEvent::ReadClosed,
            EpollEvents::RDHUP.bits(),
        ),
    ]
}

define_syscall!(EpollCreate1, |flags: EpollCreateFlags| {
    let fd_flags = if flags.contains(EpollCreateFlags::EPOLL_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(PollerObject::new(), fd_flags);
    Ok(fd)
});

fn epoll_update_impl(
    poller: ObjectRef,
    target_object: ObjectRef,
    bits: EpollEvents,
    data: u64,
) -> Result<usize, SyscallError> {
    let target_object = poll_identity_object(target_object);
    let oneshot = bits.contains(EpollEvents::ONESHOT);
    let edge_triggered = bits.contains(EpollEvents::ET);

    if target_object.clone().as_pollable().is_err() {
        return Err(SyscallError::PermissionDenied);
    }

    for (enabled, event, ready_bits) in epoll_interest_entries(bits) {
        if !enabled {
            continue;
        }
        poller.clone().as_poller()?.register_obj_with_ready_bits(
            target_object.clone(),
            event,
            data,
            ready_bits,
            oneshot,
            edge_triggered,
        );
    }

    Ok(0)
}

define_syscall!(
    EpollCtl,
    |poller: ObjectRef, op: u64, target_object: ObjectRef, event: *const LinuxEpollEvent| {
        let target_object = poll_identity_object(target_object);
        let poller = poller.as_poller()?;
        let is_registered = poller.has_registration(&target_object);

        match EpollCtlOp::try_from(op).map_err(|_| SyscallError::InvalidArguments)? {
            EpollCtlOp::Add => {
                if is_registered {
                    return Err(SyscallError::FileAlreadyExists);
                }
                if event.is_null() {
                    return Err(SyscallError::BadAddress);
                }
                let event = read_epoll_event(event)?;
                let bits = EpollEvents::from_bits_retain(event.events);
                poller.remember_registration(&target_object);
                epoll_update_impl(
                    poller
                        .self_object()
                        .expect("poller should have self object"),
                    target_object,
                    bits,
                    epoll_event_data_u64(&event),
                )
            }
            EpollCtlOp::Mod => {
                if !is_registered {
                    return Err(SyscallError::FileNotFound);
                }
                if event.is_null() {
                    return Err(SyscallError::BadAddress);
                }
                let event = read_epoll_event(event)?;
                let bits = EpollEvents::from_bits_retain(event.events);
                for existing in [
                    PollableEvent::CanBeRead,
                    PollableEvent::CanBeWritten,
                    PollableEvent::Error,
                    PollableEvent::Closed,
                    PollableEvent::ReadClosed,
                ] {
                    poller.unregister_obj(target_object.clone(), existing);
                }
                poller.remember_registration(&target_object);
                epoll_update_impl(
                    poller
                        .self_object()
                        .expect("poller should have self object"),
                    target_object,
                    bits,
                    epoll_event_data_u64(&event),
                )
            }
            EpollCtlOp::Del => {
                if !is_registered {
                    return Err(SyscallError::FileNotFound);
                }
                poller.unregister_object(target_object);
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
        } else {
            finish_block_current();
        }
    }

    let woken_events = poller.take_woken_events(maxevents);

    if !events_ptr.is_null() {
        for (index, woken) in woken_events.iter().enumerate() {
            write_epoll_event(
                unsafe { events_ptr.add(index) },
                woken.ready_bits,
                woken.data,
            )?;
        }
    }

    Ok(woken_events.len())
}

fn epoll_pwait2_timeout_ms(timeout: *const LinuxTimespec) -> Result<i32, SyscallError> {
    if timeout.is_null() {
        return Ok(-1);
    }

    let timeout = user_safe::read(timeout)?;
    if timeout.tv_sec < 0 || !(0..1_000_000_000).contains(&timeout.tv_nsec) {
        return Err(SyscallError::InvalidArguments);
    }

    let timeout_ms = (timeout.tv_sec as u128)
        .saturating_mul(1_000)
        .saturating_add(timeout.tv_nsec as u128 / 1_000_000)
        .saturating_add(u128::from(timeout.tv_nsec % 1_000_000 != 0));

    Ok(timeout_ms.min(i32::MAX as u128) as i32)
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

define_syscall!(
    EpollPwait2,
    |poller: ObjectRef,
     events_ptr: *mut LinuxEpollEvent,
     maxevents: usize,
     timeout: *const LinuxTimespec,
     _sigmask: *const u8,
     _sigsetsize: usize| {
        let timeout = epoll_pwait2_timeout_ms(timeout)?;
        epoll_wait_impl(poller, events_ptr, maxevents, timeout)
    }
);

#[cfg(test)]
mod tests {
    use crate::systemcall::{
        implementations::{
            EpollCreate1, EpollCtl, EpollPwait, EpollPwait2, EpollWait, Eventfd, Read, Shutdown,
            Socketpair, Write,
        },
        test::{
            TestLinuxEpollEvent, TestLinuxTimespec, assert_user_bytes, close_test_fd, expect_fd,
        },
        test_helpers::{
            SyscallArgs, allocate_user_test_page, assert_linux_layout, expect_errno, expect_ok,
            read_user_value, write_user_value,
        },
        utils::SyscallError,
    };

    crate::test!(
        epoll_syscalls,
        "epoll syscalls follow linux rules",
        epoll_syscalls_follow_linux_rules
    );
    crate::test!(
        epoll_pwait2_syscalls,
        "epoll_pwait2 follows linux timeout rules",
        epoll_pwait2_syscalls_follow_linux_rules
    );

    fn epoll_syscalls_follow_linux_rules() {
        const EPOLL_CTL_ADD: u64 = 1;
        const EPOLL_CTL_MOD: u64 = 3;
        const EPOLL_CTL_DEL: u64 = 2;
        const EPOLLIN: u32 = 0x001;
        const EPOLLOUT: u32 = 0x004;
        const EPOLLHUP: u32 = 0x010;
        const EPOLLRDHUP: u32 = 0x2000;
        const EPOLLET: u32 = 0x8000_0000;
        const EPOLLONESHOT: u32 = 0x4000_0000;
        const AF_UNIX: u64 = 1;
        const SOCK_STREAM: u64 = 1;
        const SHUT_WR: u64 = 1;
        assert_linux_layout::<TestLinuxEpollEvent>(12, 1);

        let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let epoll_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<EpollCreate1>());
        let event = TestLinuxEpollEvent {
            events: EPOLLOUT,
            data: 0xfeed_beef,
        };
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_ADD,
                eventfd as u64,
                (&event as *const TestLinuxEpollEvent) as u64,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        let epoll_events = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events, 4, 0, 0, 0]).call::<EpollWait>(),
            1,
        );
        let ready = read_user_value::<TestLinuxEpollEvent>(epoll_events);
        let ready_events = ready.events;
        let ready_data = ready.data;
        assert_eq!(ready_events, EPOLLOUT);
        assert_eq!(ready_data, 0xfeed_beef);
        expect_errno(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_ADD,
                eventfd as u64,
                (&event as *const TestLinuxEpollEvent) as u64,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            SyscallError::FileAlreadyExists,
        );

        let edge = TestLinuxEpollEvent {
            events: EPOLLOUT | EPOLLET,
            data: 0xabcd_1234,
        };
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_MOD,
                eventfd as u64,
                (&edge as *const TestLinuxEpollEvent) as u64,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events, 4, 0, 0, 0]).call::<EpollWait>(),
            1,
        );
        let edge_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events);
        let edge_ready_events = edge_ready.events;
        let edge_ready_data = edge_ready.data;
        assert_eq!(edge_ready_events, EPOLLOUT);
        assert_eq!(edge_ready_data, 0xabcd_1234);
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events, 4, 0, 0, 0]).call::<EpollWait>(),
            0,
        );

        let oneshot = TestLinuxEpollEvent {
            events: EPOLLOUT | EPOLLONESHOT,
            data: 7,
        };
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_MOD,
                eventfd as u64,
                (&oneshot as *const TestLinuxEpollEvent) as u64,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events, 4, 0, 0, 0]).call::<EpollPwait>(),
            1,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events, 4, 0, 0, 0]).call::<EpollWait>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_DEL, eventfd as u64, 0, 0, 0])
                .call::<EpollCtl>(),
            0,
        );
        let oneshot_only = TestLinuxEpollEvent {
            events: EPOLLONESHOT,
            data: 0x1234_5678,
        };
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_ADD,
                eventfd as u64,
                (&oneshot_only as *const TestLinuxEpollEvent) as u64,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_MOD,
                eventfd as u64,
                (&oneshot as *const TestLinuxEpollEvent) as u64,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_DEL, eventfd as u64, 0, 0, 0])
                .call::<EpollCtl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([epoll_fd as u64, 99, eventfd as u64, 0, 0, 0]).call::<EpollCtl>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_ADD, eventfd as u64, 0, 0, 0])
                .call::<EpollCtl>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([epoll_fd as u64, epoll_events, 0, 0, 0, 0]).call::<EpollWait>(),
            SyscallError::InvalidArguments,
        );

        let socketpair_page = epoll_events + 128;
        expect_ok(
            SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, socketpair_page, 0, 0]).call::<Socketpair>(),
            0,
        );
        let left = read_user_value::<i32>(socketpair_page) as usize;
        let right = read_user_value::<i32>(socketpair_page + 4) as usize;
        let socket_event = TestLinuxEpollEvent {
            events: EPOLLIN | EPOLLOUT | EPOLLHUP | EPOLLRDHUP,
            data: 0x55aa,
        };
        write_user_value(epoll_events + 192, &socket_event);
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_ADD,
                left as u64,
                epoll_events + 192,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events + 256, 4, 0, 0, 0]).call::<EpollWait>(),
            1,
        );
        let socket_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events + 256);
        let socket_ready_events = socket_ready.events;
        let socket_ready_data = socket_ready.data;
        assert_eq!(socket_ready_events, EPOLLOUT);
        assert_eq!(socket_ready_data, 0x55aa);

        write_user_value(epoll_events + 320, b"u");
        expect_ok(
            SyscallArgs::new([right as u64, epoll_events + 320, 1, 0, 0, 0]).call::<Write>(),
            1,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events + 384, 4, 0, 0, 0]).call::<EpollWait>(),
            1,
        );
        let readable_socket_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events + 384);
        let readable_socket_events = readable_socket_ready.events;
        let readable_socket_data = readable_socket_ready.data;
        assert_eq!(readable_socket_events, EPOLLIN | EPOLLOUT);
        assert_eq!(readable_socket_data, 0x55aa);
        let readable_socket_edge = TestLinuxEpollEvent {
            events: EPOLLIN | EPOLLOUT | EPOLLHUP | EPOLLRDHUP | EPOLLET,
            data: 0x77cc,
        };
        write_user_value(epoll_events + 576, &readable_socket_edge);
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_MOD,
                left as u64,
                epoll_events + 576,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events + 640, 4, 0, 0, 0]).call::<EpollWait>(),
            1,
        );
        let readable_socket_edge_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events + 640);
        let readable_socket_edge_events = readable_socket_edge_ready.events;
        let readable_socket_edge_data = readable_socket_edge_ready.data;
        assert_eq!(readable_socket_edge_events, EPOLLIN | EPOLLOUT);
        assert_eq!(readable_socket_edge_data, 0x77cc);
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events + 704, 4, 0, 0, 0]).call::<EpollWait>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([left as u64, epoll_events + 321, 1, 0, 0, 0]).call::<Read>(),
            1,
        );
        assert_user_bytes(epoll_events + 321, b"u");
        expect_ok(
            SyscallArgs::new([right as u64, SHUT_WR, 0, 0, 0, 0]).call::<Shutdown>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events + 448, 4, 0, 0, 0]).call::<EpollWait>(),
            1,
        );
        let peer_shutdown_socket_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events + 448);
        let peer_shutdown_socket_events = peer_shutdown_socket_ready.events;
        let peer_shutdown_socket_data = peer_shutdown_socket_ready.data;
        assert_eq!(peer_shutdown_socket_events, EPOLLIN | EPOLLOUT | EPOLLRDHUP);
        assert_eq!(peer_shutdown_socket_events & EPOLLHUP, 0);
        assert_eq!(peer_shutdown_socket_data, 0x77cc);
        expect_ok(
            SyscallArgs::new([left as u64, epoll_events + 449, 1, 0, 0, 0]).call::<Read>(),
            0,
        );

        expect_ok(
            SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_DEL, left as u64, 0, 0, 0])
                .call::<EpollCtl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_MOD,
                left as u64,
                epoll_events + 576,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            SyscallError::FileNotFound,
        );
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                EPOLL_CTL_ADD,
                left as u64,
                epoll_events + 192,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );
        close_test_fd(right);
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, epoll_events + 512, 4, 0, 0, 0]).call::<EpollWait>(),
            1,
        );
        let peer_closed_socket_ready = read_user_value::<TestLinuxEpollEvent>(epoll_events + 512);
        let peer_closed_socket_events = peer_closed_socket_ready.events;
        let peer_closed_socket_data = peer_closed_socket_ready.data;
        assert_eq!(
            peer_closed_socket_events,
            EPOLLIN | EPOLLOUT | EPOLLHUP | EPOLLRDHUP
        );
        assert_eq!(peer_closed_socket_data, 0x55aa);

        expect_ok(
            SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_DEL, left as u64, 0, 0, 0])
                .call::<EpollCtl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([epoll_fd as u64, EPOLL_CTL_DEL, left as u64, 0, 0, 0])
                .call::<EpollCtl>(),
            SyscallError::FileNotFound,
        );
        close_test_fd(left);
        close_test_fd(epoll_fd);
        close_test_fd(eventfd);
    }

    fn epoll_pwait2_syscalls_follow_linux_rules() {
        const EPOLLOUT: u32 = 0x004;

        let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let epoll_fd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<EpollCreate1>());
        let event = TestLinuxEpollEvent {
            events: EPOLLOUT,
            data: 0x1234_5678,
        };
        expect_ok(
            SyscallArgs::new([
                epoll_fd as u64,
                1,
                eventfd as u64,
                (&event as *const TestLinuxEpollEvent) as u64,
                0,
                0,
            ])
            .call::<EpollCtl>(),
            0,
        );

        let page = allocate_user_test_page();
        write_user_value(
            page,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 1,
            },
        );
        expect_ok(
            SyscallArgs::new([epoll_fd as u64, page + 64, 1, page, 0, 0]).call::<EpollPwait2>(),
            1,
        );
        let ready = read_user_value::<TestLinuxEpollEvent>(page + 64);
        let ready_events = ready.events;
        let ready_data = ready.data;
        assert_eq!(ready_events, EPOLLOUT);
        assert_eq!(ready_data, 0x1234_5678);

        write_user_value(
            page,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
        );
        expect_errno(
            SyscallArgs::new([epoll_fd as u64, page + 64, 1, page, 0, 0]).call::<EpollPwait2>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([epoll_fd as u64, page + 64, 0, 0, 0, 0]).call::<EpollPwait2>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([epoll_fd as u64, page + 64, 1, 1, 0, 0]).call::<EpollPwait2>(),
            SyscallError::BadAddress,
        );

        close_test_fd(epoll_fd);
        close_test_fd(eventfd);
    }
}
