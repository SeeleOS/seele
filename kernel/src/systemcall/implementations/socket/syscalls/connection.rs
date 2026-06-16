use super::*;

define_syscall!(Socket, |domain: u64, kind: u64, protocol: u64| {
    let socket: ObjectRef = if domain == AF_NETLINK {
        NetlinkSocketObject::create(kind, protocol).map_err(ObjectError::from)?
    } else if domain == AF_INET {
        InetSocketObject::create(domain, kind, protocol).map_err(ObjectError::from)?
    } else {
        UnixSocketObject::create(domain, kind, protocol).map_err(ObjectError::from)?
    };
    if (kind & SOCK_NONBLOCK) != 0 {
        let _ = socket.clone().set_flags(FileFlags::NONBLOCK);
    }
    let fd_flags = if (kind & SOCK_CLOEXEC) != 0 {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(socket, fd_flags);
    Ok(fd)
});

define_syscall!(Socketpair, |domain: u64,
                             kind: u64,
                             protocol: u64,
                             fds: *mut i32| {
    let (left, right) =
        UnixSocketObject::pair(domain, kind, protocol).map_err(ObjectError::from)?;
    let (left_fd, right_fd) = {
        let process = get_current_process();
        let mut process = process.lock();
        let fd_flags = if (kind & SOCK_CLOEXEC) != 0 {
            FdFlags::CLOEXEC
        } else {
            FdFlags::empty()
        };
        let left_fd = process.push_object_with_flags(left, fd_flags);
        let right_fd = process.push_object_with_flags(right, fd_flags);
        (left_fd, right_fd)
    };

    let fds_out = [
        i32::try_from(left_fd).map_err(|_| SyscallError::TooManyOpenFilesProcess)?,
        i32::try_from(right_fd).map_err(|_| SyscallError::TooManyOpenFilesProcess)?,
    ];
    user_safe::write(fds, &fds_out)?;

    Ok(0)
});

define_syscall!(Bind, |socket: ObjectRef,
                       address: *const u8,
                       address_len: u32| {
    let address = socket_address_bytes(address, address_len)?;
    socket
        .clone()
        .as_socket_like()?
        .bind_bytes(&address)
        .map_err(ObjectError::from)
        .map_err(SyscallError::from)?;
    Ok(0)
});

define_syscall!(Listen, |socket: ObjectRef, backlog: usize| {
    socket
        .clone()
        .as_socket_like()?
        .listen(backlog)
        .map_err(ObjectError::from)
        .map_err(SyscallError::from)?;
    Ok(0)
});

define_syscall!(Connect, |socket: ObjectRef,
                          address: *const u8,
                          address_len: u32| {
    let address = socket_address_bytes(address, address_len)?;
    socket
        .clone()
        .as_socket_like()?
        .connect_bytes(&address)
        .map_err(ObjectError::from)
        .map_err(SyscallError::from)?;
    Ok(0)
});

define_syscall!(Accept, |socket: ObjectRef,
                         address: *mut u8,
                         address_len_ptr: *mut u32| {
    if !address.is_null() && address_len_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let fd = accept_socket(socket, 0)?;
    if !address_len_ptr.is_null() {
        let accepted = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
        let name = accepted
            .as_socket_like()?
            .getpeername_bytes()
            .map_err(ObjectError::from)?;
        write_socket_name(address, address_len_ptr, &name)?;
    }
    Ok(fd)
});

define_syscall!(Accept4, |socket: ObjectRef,
                          address: *mut u8,
                          address_len_ptr: *mut u32,
                          flags: u32| {
    if !address.is_null() && address_len_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let fd = accept_socket(socket, flags)?;
    if !address_len_ptr.is_null() {
        let accepted = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
        let name = accepted
            .as_socket_like()?
            .getpeername_bytes()
            .map_err(ObjectError::from)?;
        write_socket_name(address, address_len_ptr, &name)?;
    }
    Ok(fd)
});

define_syscall!(Shutdown, |socket: ObjectRef, how: u64| {
    socket
        .as_socket_like()?
        .shutdown(how)
        .map_err(ObjectError::from)?;
    Ok(0)
});

#[cfg(test)]
mod tests {
    use crate::systemcall::test::*;

    crate::test!(
        socket_bind_connect_accept_syscalls,
        "bind listen connect and accept4 follow linux socket rules",
        socket_bind_connect_accept_syscalls_follow_linux_rules
    );
    fn socket_bind_connect_accept_syscalls_follow_linux_rules() {
        const AF_INET: u64 = 2;
        const AF_UNIX: u64 = 1;
        const SOCK_STREAM: u64 = 1;
        const SOCK_DGRAM: u64 = 2;
        const SOCK_NONBLOCK: u64 = 0o0004000;
        const SOCK_CLOEXEC: u64 = 0o2000000;

        assert_linux_layout::<TestLinuxSockAddrUn>(110, 2);
        assert_linux_layout::<TestLinuxSockAddrIn>(16, 2);

        let page = allocate_user_test_page();
        let socket_path = b"/tmp/accept4-linux.sock\0";
        let missing_socket_path = b"/tmp/accept4-missing.sock\0";
        write_user_value(page, socket_path);
        write_user_value(page + 384, missing_socket_path);

        let mut unix_addr = TestLinuxSockAddrUn::default();
        unix_addr.sun_family = AF_UNIX as u16;
        unix_addr.sun_path[..socket_path.len()].copy_from_slice(socket_path);
        write_user_value(page + 128, &unix_addr);
        let mut missing_unix_addr = TestLinuxSockAddrUn::default();
        missing_unix_addr.sun_family = AF_UNIX as u16;
        missing_unix_addr.sun_path[..missing_socket_path.len()]
            .copy_from_slice(missing_socket_path);
        write_user_value(page + 640, &missing_unix_addr);

        let server = expect_fd(
            SyscallArgs::new([AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0, 0, 0, 0]).call::<Socket>(),
        );
        expect_ok(
            SyscallArgs::new([server as u64, page + 128, 110, 0, 0, 0]).call::<Bind>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([server as u64, page + 128, 110, 0, 0, 0]).call::<Bind>(),
            SyscallError::InvalidArguments,
        );
        let occupied =
            expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
        expect_errno(
            SyscallArgs::new([occupied as u64, page + 128, 110, 0, 0, 0]).call::<Bind>(),
            SyscallError::AddressInUse,
        );
        expect_ok(
            SyscallArgs::new([server as u64, 0, 0, 0, 0, 0]).call::<Listen>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([server as u64, page + 256, page + 264, SOCK_NONBLOCK, 0, 0])
                .call::<Accept4>(),
            SyscallError::TryAgain,
        );

        let client =
            expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
        expect_ok(
            SyscallArgs::new([client as u64, page + 128, 110, 0, 0, 0]).call::<Connect>(),
            0,
        );

        write_user_value(page + 264, &2u32);
        let accepted = expect_fd(
            SyscallArgs::new([
                server as u64,
                page + 256,
                page + 264,
                SOCK_NONBLOCK | SOCK_CLOEXEC,
                0,
                0,
            ])
            .call::<Accept4>(),
        );
        assert_fd_flags(accepted, FdFlags::CLOEXEC);
        assert_object_flags(accepted, FileFlags::NONBLOCK);
        assert_eq!(read_user_value::<u32>(page + 264), 2);
        let peer = read_user_value::<TestLinuxSockAddrUn>(page + 256);
        assert_eq!(peer.sun_family, AF_UNIX as u16);

        let rebound =
            expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
        expect_errno(
            SyscallArgs::new([rebound as u64, page + 128, 1, 0, 0, 0]).call::<Bind>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([rebound as u64, 0, 110, 0, 0, 0]).call::<Bind>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([rebound as u64, 0, 0, 0, 0, 0]).call::<Connect>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([rebound as u64, page + 640, 110, 0, 0, 0]).call::<Connect>(),
            SyscallError::ConnectionRefused,
        );

        let unix_dgram =
            expect_fd(SyscallArgs::new([AF_UNIX, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
        expect_errno(
            SyscallArgs::new([unix_dgram as u64, 1, 0, 0, 0, 0]).call::<Listen>(),
            SyscallError::InvalidArguments,
        );

        let inet_stream = expect_fd(
            SyscallArgs::new([AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0, 0, 0, 0]).call::<Socket>(),
        );
        let inet_any = TestLinuxSockAddrIn {
            sin_family: AF_INET as u16,
            sin_port: 0,
            sin_addr: [0, 0, 0, 0],
            sin_zero: [0; 8],
        };
        write_user_value(page + 512, &inet_any);
        expect_errno(
            SyscallArgs::new([inet_stream as u64, page + 512, 16, 0, 0, 0]).call::<Bind>(),
            SyscallError::AddressNotAvailable,
        );
        expect_errno(
            SyscallArgs::new([inet_stream as u64, 1, 0, 0, 0, 0]).call::<Listen>(),
            SyscallError::AddressNotAvailable,
        );
        expect_errno(
            SyscallArgs::new([inet_stream as u64, page + 512, 16, 0, 0, 0]).call::<Connect>(),
            SyscallError::ConnectionRefused,
        );

        let inet_dgram =
            expect_fd(SyscallArgs::new([AF_INET, SOCK_DGRAM, 0, 0, 0, 0]).call::<Socket>());
        expect_errno(
            SyscallArgs::new([inet_dgram as u64, 1, 0, 0, 0, 0]).call::<Listen>(),
            SyscallError::OperationNotSupported,
        );
        expect_errno(
            SyscallArgs::new([inet_dgram as u64, page + 512, 16, 0, 0, 0]).call::<Connect>(),
            SyscallError::ConnectionRefused,
        );
        expect_errno(
            SyscallArgs::new([inet_dgram as u64, page + 256, page + 264, 0, 0, 0])
                .call::<Accept4>(),
            SyscallError::OperationNotSupported,
        );

        close_test_fd(inet_dgram);
        close_test_fd(inet_stream);
        close_test_fd(unix_dgram);
        close_test_fd(rebound);
        close_test_fd(occupied);
        close_test_fd(accepted);
        close_test_fd(client);
        close_test_fd(server);
    }
}
