use super::*;

define_syscall!(Setsockopt, |socket: ObjectRef,
                             level: i32,
                             option_name: i32,
                             option_value: *const u8,
                             option_len: u32| {
    if option_len > 0 && option_value.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let option_value = if option_len == 0 {
        Vec::new()
    } else {
        user_safe::read_buffer(option_value, option_len as usize)?
    };
    socket
        .as_socket_like()?
        .setsockopt(level as u64, option_name as u64, option_value.as_slice())
        .map_err(ObjectError::from)?;

    Ok(0)
});

define_syscall!(
    Getsockopt,
    |socket: ObjectRef,
     level: i32,
     option_name: i32,
     option_value: *mut u8,
     option_len_ptr: *mut u32| {
        if option_len_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let option_len = user_safe::read(option_len_ptr)? as usize;
        let value = socket
            .as_socket_like()?
            .getsockopt(level as u64, option_name as u64, option_len)
            .map_err(ObjectError::from)?;

        if option_value.is_null() {
            if option_len != 0 && !value.is_empty() {
                return Err(SyscallError::BadAddress);
            }
        } else if !value.is_empty() {
            let copy_len = option_len.min(value.len());
            user_safe::write(option_value, &value[..copy_len])?;
        }

        if option_value.is_null() && option_len == 0 {
            user_safe::write(option_len_ptr, &(value.len() as u32))?;
            return Ok(0);
        }

        if option_value.is_null() && value.is_empty() {
            user_safe::write(option_len_ptr, &(value.len() as u32))?;
            return Ok(0);
        }

        if option_value.is_null() && option_len != 0 {
            return Err(SyscallError::BadAddress);
        }
        user_safe::write(option_len_ptr, &(value.len() as u32))?;

        Ok(0)
    }
);

define_syscall!(
    Getsockname,
    |socket: ObjectRef, address: *mut u8, address_len_ptr: *mut u32| {
        let name = socket
            .as_socket_like()?
            .getsockname_bytes()
            .map_err(ObjectError::from)?;
        write_socket_name(address, address_len_ptr, &name)?;
        Ok(0)
    }
);

define_syscall!(
    Getpeername,
    |socket: ObjectRef, address: *mut u8, address_len_ptr: *mut u32| {
        let name = socket
            .as_socket_like()?
            .getpeername_bytes()
            .map_err(ObjectError::from)?;
        write_socket_name(address, address_len_ptr, &name)?;
        Ok(0)
    }
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::{Signal, SignalAction, SignalHandlingType, Signals};
    use crate::systemcall::implementations::{
        Getpeername, Getsockname, Getsockopt, Poll, Read, Setsockopt, Shutdown, Socket, Socketpair,
        Write,
    };
    use crate::systemcall::test::*;

    crate::test!(
        socket_name_and_shutdown_syscalls,
        "socketpair shutdown getsockname and getpeername follow linux rules",
        socket_name_and_shutdown_syscalls_follow_linux_rules
    );
    fn socket_name_and_shutdown_syscalls_follow_linux_rules() {
        const AF_INET: u64 = 2;
        const AF_NETLINK: u64 = 16;
        const AF_UNIX: u64 = 1;
        const SOL_SOCKET: u64 = 1;
        const SOL_TCP: u64 = 6;
        const SOCK_STREAM: u64 = 1;
        const SOCK_DGRAM: u64 = 2;
        const SOCK_RAW: u64 = 3;
        const SOCK_NONBLOCK: u64 = 0o0004000;
        const SOCK_CLOEXEC: u64 = 0o2000000;
        const SHUT_RD: u64 = 0;
        const SHUT_WR: u64 = 1;
        const SHUT_RDWR: u64 = 2;
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        const POLLHUP: i16 = 0x010;
        const POLLRDHUP: i16 = 0x2000;
        const SO_TYPE: u64 = 3;
        const SO_ERROR: u64 = 4;
        const SO_SNDBUF: u64 = 7;
        const SO_PRIORITY: u64 = 12;
        const SO_PASSCRED: u64 = 16;
        const SO_PEERCRED: u64 = 17;
        const SO_ACCEPTCONN: u64 = 30;
        const SO_PROTOCOL: u64 = 38;
        const SO_DOMAIN: u64 = 39;
        const SO_PEERPIDFD: u64 = 77;
        const TCP_NODELAY: u64 = 1;

        assert_linux_layout::<TestLinuxSockAddrUn>(110, 2);
        assert_linux_layout::<TestLinuxSockAddrIn>(16, 2);

        let saved = CredentialSnapshot::save_current();
        let page = allocate_user_test_page();
        let process = get_current_process();
        let (saved_exit_status, saved_sigpipe_action, saved_pending_signals) = {
            let mut process = process.lock();
            let saved_exit_status = process.exit_status;
            let saved_sigpipe_action = process.signal_actions[Signal::SIGPIPE.index()].clone();
            let saved_pending_signals = process.pending_signals;
            process.exit_status = None;
            process
                .pending_signals
                .remove(Signals::from(Signal::SIGPIPE));
            process.pending_signal_info[Signal::SIGPIPE.index()] = None;
            process.signal_actions[Signal::SIGPIPE.index()] = SignalAction {
                handling_type: SignalHandlingType::Ignore,
                ..SignalAction::default()
            };
            (
                saved_exit_status,
                saved_sigpipe_action,
                saved_pending_signals,
            )
        };

        let socketpair_fds_page = page;
        expect_ok(
            SyscallArgs::new([
                AF_UNIX,
                SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
                0,
                socketpair_fds_page,
                0,
                0,
            ])
            .call::<Socketpair>(),
            0,
        );
        let [left_fd, right_fd] = read_user_value::<[i32; 2]>(socketpair_fds_page);
        let left_fd = usize::try_from(left_fd).expect("socketpair left fd should be non-negative");
        let right_fd =
            usize::try_from(right_fd).expect("socketpair right fd should be non-negative");
        assert_fd_flags(left_fd, FdFlags::CLOEXEC);
        assert_fd_flags(right_fd, FdFlags::CLOEXEC);
        assert_object_flags(left_fd, FileFlags::NONBLOCK);
        assert_object_flags(right_fd, FileFlags::NONBLOCK);

        let pollfds_page = page + 48;
        write_user_value(
            pollfds_page,
            &[TestLinuxPollFd {
                fd: left_fd as i32,
                events: POLLIN | POLLOUT | POLLHUP,
                revents: -1,
            }],
        );
        expect_ok(
            SyscallArgs::new([pollfds_page, 1, 0, 0, 0, 0]).call::<Poll>(),
            1,
        );
        let initial_poll = read_user_value::<TestLinuxPollFd>(pollfds_page);
        assert_eq!(initial_poll.revents & POLLOUT, POLLOUT);
        assert_eq!(initial_poll.revents & POLLIN, 0);
        assert_eq!(initial_poll.revents & POLLHUP, 0);

        write_user_value(page + 56, b"z");
        expect_ok(
            SyscallArgs::new([right_fd as u64, page + 56, 1, 0, 0, 0]).call::<Write>(),
            1,
        );
        write_user_value(
            pollfds_page,
            &[TestLinuxPollFd {
                fd: left_fd as i32,
                events: POLLIN | POLLOUT | POLLHUP,
                revents: 0,
            }],
        );
        expect_ok(
            SyscallArgs::new([pollfds_page, 1, 0, 0, 0, 0]).call::<Poll>(),
            1,
        );
        let readable_poll = read_user_value::<TestLinuxPollFd>(pollfds_page);
        assert_eq!(readable_poll.revents & POLLIN, POLLIN);
        assert_eq!(readable_poll.revents & POLLOUT, POLLOUT);
        assert_eq!(readable_poll.revents & POLLHUP, 0);
        expect_ok(
            SyscallArgs::new([left_fd as u64, page + 57, 1, 0, 0, 0]).call::<Read>(),
            1,
        );
        assert_user_bytes(page + 57, b"z");

        write_user_value(page + 64, &4u32);
        expect_ok(
            SyscallArgs::new([
                left_fd as u64,
                SOL_SOCKET,
                SO_PEERPIDFD,
                page + 72,
                page + 64,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 64), 4);
        let socketpair_peer_pidfd = read_user_value::<i32>(page + 72);
        let socketpair_peer_pidfd =
            usize::try_from(socketpair_peer_pidfd).expect("peer pidfd should be non-negative");
        assert_fd_flags(socketpair_peer_pidfd, FdFlags::CLOEXEC);
        let current_pid = get_current_process().lock().pid.0;
        let socketpair_peer_pidfd_object = get_object_current_process(socketpair_peer_pidfd as u64)
            .expect("peer pidfd should resolve")
            .as_pidfd()
            .expect("SO_PEERPIDFD should install a pidfd");
        assert_eq!(socketpair_peer_pidfd_object.pid(), current_pid);
        write_user_value(
            page + 96,
            &[TestLinuxPollFd {
                fd: socketpair_peer_pidfd as i32,
                events: POLLIN,
                revents: -1,
            }],
        );
        expect_ok(
            SyscallArgs::new([page + 96, 1, 0, 0, 0, 0]).call::<Poll>(),
            0,
        );
        assert_eq!(read_user_value::<TestLinuxPollFd>(page + 96).revents, 0);
        close_test_fd(socketpair_peer_pidfd);

        write_user_value(page + 64, &111u32);
        expect_ok(
            SyscallArgs::new([left_fd as u64, page + 128, page + 64, 0, 0, 0])
                .call::<Getsockname>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 64), 2);
        let local_un = read_user_value::<TestLinuxSockAddrUn>(page + 128);
        assert_eq!(local_un.sun_family, AF_UNIX as u16);
        assert!(local_un.sun_path.iter().all(|&byte| byte == 0));

        write_user_value(page + 80, &111u32);
        expect_ok(
            SyscallArgs::new([left_fd as u64, page + 256, page + 80, 0, 0, 0])
                .call::<Getpeername>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 80), 2);
        let peer_un = read_user_value::<TestLinuxSockAddrUn>(page + 256);
        assert_eq!(peer_un.sun_family, AF_UNIX as u16);
        assert!(peer_un.sun_path.iter().all(|&byte| byte == 0));

        write_user_value(page + 96, &1u32);
        expect_ok(
            SyscallArgs::new([left_fd as u64, page + 384, page + 96, 0, 0, 0])
                .call::<Getpeername>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 96), 2);
        assert_user_bytes(page + 384, &[AF_UNIX as u8]);

        expect_errno(
            SyscallArgs::new([left_fd as u64, page + 384, 0, 0, 0, 0]).call::<Getsockname>(),
            SyscallError::BadAddress,
        );
        write_user_value(page + 96, &4u32);
        expect_errno(
            SyscallArgs::new([left_fd as u64, 0, page + 96, 0, 0, 0]).call::<Getsockname>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([left_fd as u64, page + 384, page + 96, 99, 0, 0]).call::<Shutdown>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([left_fd as u64, SHUT_RD, 0, 0, 0, 0]).call::<Shutdown>(),
            0,
        );
        write_user_value(page + 512, b"x");
        expect_errno(
            SyscallArgs::new([right_fd as u64, page + 512, 1, 0, 0, 0]).call::<Write>(),
            SyscallError::BrokenPipe,
        );
        expect_ok(
            SyscallArgs::new([right_fd as u64, SHUT_WR, 0, 0, 0, 0]).call::<Shutdown>(),
            0,
        );
        write_user_value(
            pollfds_page,
            &[TestLinuxPollFd {
                fd: left_fd as i32,
                events: POLLIN | POLLOUT | POLLHUP | POLLRDHUP,
                revents: 0,
            }],
        );
        expect_ok(
            SyscallArgs::new([pollfds_page, 1, 0, 0, 0, 0]).call::<Poll>(),
            1,
        );
        let peer_shutdown_poll = read_user_value::<TestLinuxPollFd>(pollfds_page);
        assert_eq!(peer_shutdown_poll.revents & POLLIN, POLLIN);
        assert_eq!(peer_shutdown_poll.revents & POLLOUT, POLLOUT);
        assert_eq!(peer_shutdown_poll.revents & POLLHUP, 0);
        assert_eq!(peer_shutdown_poll.revents & POLLRDHUP, POLLRDHUP);
        expect_ok(
            SyscallArgs::new([left_fd as u64, page + 58, 1, 0, 0, 0]).call::<Read>(),
            0,
        );
        write_user_value(page + 768, b"q");
        expect_errno(
            SyscallArgs::new([right_fd as u64, page + 768, 1, 0, 0, 0]).call::<Write>(),
            SyscallError::BrokenPipe,
        );
        write_user_value(page + 776, b"r");
        expect_ok(
            SyscallArgs::new([left_fd as u64, page + 776, 1, 0, 0, 0]).call::<Write>(),
            1,
        );
        expect_ok(
            SyscallArgs::new([right_fd as u64, SHUT_RDWR, 0, 0, 0, 0]).call::<Shutdown>(),
            0,
        );

        expect_errno(
            SyscallArgs::new([AF_INET, SOCK_STREAM, 0, socketpair_fds_page, 0, 0])
                .call::<Socketpair>(),
            SyscallError::AddressFamilyNotSupported,
        );
        expect_errno(
            SyscallArgs::new([AF_UNIX, SOCK_STREAM, 1, socketpair_fds_page, 0, 0])
                .call::<Socketpair>(),
            SyscallError::ProtocolNotSupported,
        );
        expect_errno(
            SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socketpair>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([AF_UNIX, 7, 0, socketpair_fds_page, 0, 0]).call::<Socketpair>(),
            SyscallError::ProtocolNotSupported,
        );

        let unix_socket = expect_fd(
            SyscallArgs::new([
                AF_UNIX,
                SOCK_DGRAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
                0,
                0,
                0,
                0,
            ])
            .call::<Socket>(),
        );
        assert_fd_flags(unix_socket, FdFlags::CLOEXEC);
        assert_object_flags(unix_socket, FileFlags::NONBLOCK);
        write_user_value(page + 896, &1i32);
        expect_ok(
            SyscallArgs::new([
                unix_socket as u64,
                SOL_SOCKET,
                SO_PASSCRED,
                page + 896,
                4,
                0,
            ])
            .call::<Setsockopt>(),
            0,
        );
        write_user_value(page + 904, &4u32);
        expect_ok(
            SyscallArgs::new([
                unix_socket as u64,
                SOL_SOCKET,
                SO_PASSCRED,
                page + 912,
                page + 904,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 904), 4);
        assert_eq!(read_user_value::<i32>(page + 912), 1);
        write_user_value(page + 920, &4u32);
        expect_ok(
            SyscallArgs::new([
                unix_socket as u64,
                SOL_SOCKET,
                SO_TYPE,
                page + 928,
                page + 920,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 920), 4);
        assert_eq!(read_user_value::<i32>(page + 928), SOCK_DGRAM as i32);
        write_user_value(page + 936, &4u32);
        expect_ok(
            SyscallArgs::new([
                unix_socket as u64,
                SOL_SOCKET,
                SO_DOMAIN,
                page + 944,
                page + 936,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 944), AF_UNIX as i32);
        write_user_value(page + 952, &12u32);
        expect_ok(
            SyscallArgs::new([
                unix_socket as u64,
                SOL_SOCKET,
                SO_PEERCRED,
                page + 960,
                page + 952,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 952), 12);
        let peercred_words = read_user_value::<[u32; 3]>(page + 960);
        let current = get_current_process();
        let current_locked = current.lock();
        assert_eq!(peercred_words[0], current_locked.pid.0 as u32);
        assert_eq!(peercred_words[1], current_locked.effective_uid);
        assert_eq!(peercred_words[2], current_locked.effective_gid);
        drop(current_locked);
        expect_errno(
            SyscallArgs::new([
                unix_socket as u64,
                SOL_SOCKET,
                SO_PEERCRED,
                page + 960,
                0,
                0,
            ])
            .call::<Getsockopt>(),
            SyscallError::BadAddress,
        );
        write_user_value(page + 952, &3u32);
        expect_errno(
            SyscallArgs::new([
                unix_socket as u64,
                SOL_SOCKET,
                SO_TYPE,
                page + 928,
                page + 952,
                0,
            ])
            .call::<Getsockopt>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([unix_socket as u64, SOL_SOCKET, SO_TYPE, 0, page + 920, 0])
                .call::<Getsockopt>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([unix_socket as u64, SOL_SOCKET, SO_TYPE, page + 928, 0, 0])
                .call::<Getsockopt>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([unix_socket as u64, SOL_SOCKET, SO_PASSCRED, 0, 4, 0])
                .call::<Setsockopt>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([unix_socket as u64, SOL_SOCKET, SO_ERROR, page + 896, 4, 0])
                .call::<Setsockopt>(),
            SyscallError::InvalidArguments,
        );

        let inet_socket =
            expect_fd(SyscallArgs::new([AF_INET, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
        write_user_value(page + 96, &111u32);
        expect_ok(
            SyscallArgs::new([inet_socket as u64, page + 640, page + 96, 0, 0, 0])
                .call::<Getsockname>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page + 96), 16);
        let inet_name = read_user_value::<TestLinuxSockAddrIn>(page + 640);
        assert_eq!(inet_name.sin_family, AF_INET as u16);
        assert_eq!(inet_name.sin_port, 0);
        assert_eq!(inet_name.sin_addr, [0, 0, 0, 0]);
        assert_eq!(inet_name.sin_zero, [0; 8]);

        expect_errno(
            SyscallArgs::new([inet_socket as u64, page + 768, page + 96, 0, 0, 0])
                .call::<Getpeername>(),
            SyscallError::NotConnected,
        );
        write_user_value(page + 968, &4u32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_TYPE,
                page + 976,
                page + 968,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 976), SOCK_DGRAM as i32);
        write_user_value(page + 984, &4u32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_PROTOCOL,
                page + 992,
                page + 984,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 992), 17);
        write_user_value(page + 1000, &4u32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_ACCEPTCONN,
                page + 1008,
                page + 1000,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1008), 0);
        write_user_value(page + 1016, &4u32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_DOMAIN,
                page + 1024,
                page + 1016,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1024), AF_INET as i32);
        write_user_value(page + 1032, &4u32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_ERROR,
                page + 1040,
                page + 1032,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1040), 0);
        write_user_value(page + 1048, &8192i32);
        expect_ok(
            SyscallArgs::new([inet_socket as u64, SOL_SOCKET, SO_SNDBUF, page + 1048, 4, 0])
                .call::<Setsockopt>(),
            0,
        );
        write_user_value(page + 1052, &6i32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_PRIORITY,
                page + 1052,
                4,
                0,
            ])
            .call::<Setsockopt>(),
            0,
        );
        write_user_value(page + 1060, &4u32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_PRIORITY,
                page + 1068,
                page + 1060,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1068), 6);
        {
            let process = get_current_process();
            let mut process = process.lock();
            process.effective_uid = 1000;
            process.capability_effective = [0; 2];
        }
        write_user_value(page + 1052, &7i32);
        expect_errno(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_PRIORITY,
                page + 1052,
                4,
                0,
            ])
            .call::<Setsockopt>(),
            SyscallError::PermissionDenied,
        );
        write_user_value(page + 1052, &(-1i32));
        expect_errno(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_PRIORITY,
                page + 1052,
                4,
                0,
            ])
            .call::<Setsockopt>(),
            SyscallError::PermissionDenied,
        );
        write_user_value(page + 1060, &4u32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_SOCKET,
                SO_PRIORITY,
                page + 1068,
                page + 1060,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1068), 6);
        saved.restore();
        write_user_value(page + 1056, &4i32);
        expect_ok(
            SyscallArgs::new([inet_socket as u64, SOL_TCP, TCP_NODELAY, page + 1056, 4, 0])
                .call::<Setsockopt>(),
            0,
        );
        write_user_value(page + 1064, &4u32);
        expect_ok(
            SyscallArgs::new([
                inet_socket as u64,
                SOL_TCP,
                TCP_NODELAY,
                page + 1072,
                page + 1064,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1072), 1);
        expect_errno(
            SyscallArgs::new([inet_socket as u64, SOL_TCP, 99, page + 1056, 4, 0])
                .call::<Setsockopt>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([inet_socket as u64, SOL_TCP, 99, page + 1072, page + 1064, 0])
                .call::<Getsockopt>(),
            SyscallError::InvalidArguments,
        );

        let netlink_socket = expect_fd(
            SyscallArgs::new([
                AF_NETLINK,
                SOCK_RAW | SOCK_NONBLOCK | SOCK_CLOEXEC,
                0,
                0,
                0,
                0,
            ])
            .call::<Socket>(),
        );
        assert_fd_flags(netlink_socket, FdFlags::CLOEXEC);
        assert_object_flags(netlink_socket, FileFlags::NONBLOCK);
        write_user_value(page + 1080, &4u32);
        expect_ok(
            SyscallArgs::new([
                netlink_socket as u64,
                SOL_SOCKET,
                SO_TYPE,
                page + 1088,
                page + 1080,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1088), SOCK_RAW as i32);
        write_user_value(page + 1096, &4u32);
        expect_ok(
            SyscallArgs::new([
                netlink_socket as u64,
                SOL_SOCKET,
                SO_DOMAIN,
                page + 1104,
                page + 1096,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1104), AF_NETLINK as i32);
        write_user_value(page + 1112, &4u32);
        expect_ok(
            SyscallArgs::new([
                netlink_socket as u64,
                SOL_SOCKET,
                SO_PROTOCOL,
                page + 1120,
                page + 1112,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1120), 0);
        write_user_value(page + 1128, &1i32);
        expect_ok(
            SyscallArgs::new([
                netlink_socket as u64,
                SOL_SOCKET,
                SO_PASSCRED,
                page + 1128,
                4,
                0,
            ])
            .call::<Setsockopt>(),
            0,
        );
        write_user_value(page + 1136, &4u32);
        expect_ok(
            SyscallArgs::new([
                netlink_socket as u64,
                SOL_SOCKET,
                SO_PASSCRED,
                page + 1144,
                page + 1136,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1144), 1);
        write_user_value(page + 1152, &4u32);
        expect_ok(
            SyscallArgs::new([
                netlink_socket as u64,
                SOL_SOCKET,
                SO_PRIORITY,
                page + 1160,
                page + 1152,
                0,
            ])
            .call::<Getsockopt>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 1160), 0);

        expect_errno(
            SyscallArgs::new([99, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>(),
            SyscallError::AddressFamilyNotSupported,
        );
        expect_errno(
            SyscallArgs::new([AF_INET, SOCK_STREAM, 17, 0, 0, 0]).call::<Socket>(),
            SyscallError::ProtocolNotSupported,
        );
        expect_errno(
            SyscallArgs::new([AF_NETLINK, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>(),
            SyscallError::ProtocolNotSupported,
        );

        close_test_fd(netlink_socket);
        close_test_fd(inet_socket);
        close_test_fd(unix_socket);
        close_test_fd(right_fd);
        close_test_fd(left_fd);

        {
            let mut process = process.lock();
            process.exit_status = saved_exit_status;
            process.pending_signals = saved_pending_signals;
            process.pending_signal_info[Signal::SIGPIPE.index()] = None;
            process.signal_actions[Signal::SIGPIPE.index()] = saved_sigpipe_action;
        }
    }
}
