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
}
