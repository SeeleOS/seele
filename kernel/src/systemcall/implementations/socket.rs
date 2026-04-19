use crate::{
    define_syscall,
    memory::user_safe,
    object::Object,
    object::misc::ObjectRef,
    process::manager::get_current_process,
    socket::{AF_UNIX, SOCK_NONBLOCK},
    systemcall::utils::{SyscallError, SyscallImpl},
};
use alloc::{string::String, vec::Vec};
use core::slice;

#[repr(C)]
struct LinuxSockAddrUn {
    sun_family: u16,
    sun_path: [u8; 108],
}

fn path_from_sockaddr(address: *const u8, address_len: u32) -> Result<String, SyscallError> {
    if address.is_null() || address_len < 2 {
        return Err(SyscallError::BadAddress);
    }
    let addr = unsafe { &*(address as *const LinuxSockAddrUn) };
    if addr.sun_family != AF_UNIX as u16 {
        return Err(SyscallError::AddressFamilyNotSupported);
    }
    let path_len = (address_len as usize)
        .saturating_sub(2)
        .min(addr.sun_path.len());
    if path_len == 0 {
        return Err(SyscallError::InvalidArguments);
    }

    if addr.sun_path[0] == 0 {
        if path_len <= 1 {
            return Err(SyscallError::InvalidArguments);
        }
        return Ok(String::from_utf8_lossy(&addr.sun_path[..path_len]).into_owned());
    }

    let len = addr.sun_path[..path_len]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(path_len);
    if len == 0 {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(String::from_utf8_lossy(&addr.sun_path[..len]).into_owned())
}

define_syscall!(Socket, |domain: u64, kind: u64, protocol: u64| {
    let socket = crate::socket::UnixSocketObject::create(domain, kind, protocol)
        .map_err(crate::object::error::ObjectError::from)?;
    if (kind & SOCK_NONBLOCK) != 0 {
        let _ = socket.clone().set_flags(crate::object::FileFlags::NONBLOCK);
    }
    let fd = get_current_process().lock().push_object(socket);
    Ok(fd)
});

define_syscall!(Socketpair, |domain: u64,
                             kind: u64,
                             protocol: u64,
                             fds: *mut i32| {
    let (left, right) = crate::socket::UnixSocketObject::pair(domain, kind, protocol)
        .map_err(crate::object::error::ObjectError::from)?;
    let (left_fd, right_fd) = {
        let process = get_current_process();
        let mut process = process.lock();
        let left_fd = process.push_object(left);
        let right_fd = process.push_object(right);
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
    let path = path_from_sockaddr(address, address_len)?;
    let result = socket
        .as_unix_socket()?
        .bind(path)
        .map_err(crate::object::error::ObjectError::from);
    result?;
    Ok(0)
});

define_syscall!(Listen, |socket: ObjectRef, backlog: usize| {
    let result = socket
        .as_unix_socket()?
        .listen(backlog)
        .map_err(crate::object::error::ObjectError::from);
    result?;
    Ok(0)
});

define_syscall!(Connect, |socket: ObjectRef,
                          address: *const u8,
                          address_len: u32| {
    let path = path_from_sockaddr(address, address_len)?;
    let result = socket
        .as_unix_socket()?
        .connect(path.clone())
        .map_err(crate::object::error::ObjectError::from);
    result?;
    Ok(0)
});

define_syscall!(Accept, |socket: ObjectRef,
                         address: *mut u8,
                         address_len_ptr: *mut u32| {
    let fd = socket
        .as_unix_socket()?
        .accept()
        .map_err(crate::object::error::ObjectError::from)?;
    if !address_len_ptr.is_null() {
        let accepted = crate::object::misc::get_object_current_process(fd as u64)
            .map_err(SyscallError::from)?;
        let name = accepted
            .as_unix_socket()?
            .getpeername_bytes()
            .map_err(crate::object::error::ObjectError::from)?;
        let requested_len = unsafe { *address_len_ptr as usize };
        let copy_len = requested_len.min(name.len());
        if copy_len > 0 {
            user_safe::write(address, &name[..copy_len])?;
        }
        user_safe::write(address_len_ptr, &(name.len() as u32))?;
    }
    Ok(fd)
});

define_syscall!(Accept4, |socket: ObjectRef,
                          address: *mut u8,
                          address_len_ptr: *mut u32,
                          flags: u32| {
    let fd = socket
        .as_unix_socket()?
        .accept()
        .map_err(crate::object::error::ObjectError::from)?;
    if !address_len_ptr.is_null() {
        let accepted = crate::object::misc::get_object_current_process(fd as u64)
            .map_err(SyscallError::from)?;
        let name = accepted
            .as_unix_socket()?
            .getpeername_bytes()
            .map_err(crate::object::error::ObjectError::from)?;
        let requested_len = unsafe { *address_len_ptr as usize };
        let copy_len = requested_len.min(name.len());
        if copy_len > 0 {
            user_safe::write(address, &name[..copy_len])?;
        }
        user_safe::write(address_len_ptr, &(name.len() as u32))?;
    }
    let accepted =
        crate::object::misc::get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    let mut file_flags = crate::object::FileFlags::empty();
    if (flags & SOCK_NONBLOCK as u32) != 0 {
        file_flags.insert(crate::object::FileFlags::NONBLOCK);
    }
    let _ = accepted.set_flags(file_flags);
    Ok(fd)
});

define_syscall!(Sendto, |socket: ObjectRef,
                         buffer: *const u8,
                         len: usize,
                         _flags: u64,
                         address: *const u8,
                         address_len: u32| {
    if len > 0 && buffer.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let socket = socket.as_unix_socket()?;
    if !address.is_null() {
        let path = path_from_sockaddr(address, address_len)?;
        if matches!(
            &*socket.state.lock(),
            crate::socket::UnixSocketState::Unbound
        ) {
            socket
                .connect(path)
                .map_err(crate::object::error::ObjectError::from)?;
        }
    }

    let buffer = unsafe { slice::from_raw_parts(buffer, len) };
    let written = socket
        .write_socket(buffer)
        .map_err(crate::object::error::ObjectError::from)?;

    Ok(written)
});

define_syscall!(
    Recvfrom,
    |socket: ObjectRef,
     buffer: *mut u8,
     len: usize,
     _flags: u64,
     address: *mut u8,
     address_len_ptr: *mut u32| {
        if len > 0 && buffer.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let socket = socket.as_unix_socket()?;
        let mut data = Vec::new();
        data.resize(len, 0);
        let read = socket
            .read_socket(&mut data)
            .map_err(crate::object::error::ObjectError::from)?;

        if read > 0 {
            user_safe::write(buffer, &data[..read])?;
        }

        if !address.is_null() {
            if address_len_ptr.is_null() {
                return Err(SyscallError::BadAddress);
            }
            let name = socket
                .getpeername_bytes()
                .map_err(crate::object::error::ObjectError::from)?;
            let requested_len = unsafe { *address_len_ptr as usize };
            let copy_len = requested_len.min(name.len());
            if copy_len > 0 {
                user_safe::write(address, &name[..copy_len])?;
            }
            user_safe::write(address_len_ptr, &(name.len() as u32))?;
        }

        Ok(read)
    }
);

define_syscall!(Sendmsg, |socket: ObjectRef,
                          msg: *const relibc_msg_hdr,
                          _flags: u64| {
    if msg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let msg = unsafe { &*msg };
    if msg.msg_iovlen > isize::MAX as usize {
        return Err(SyscallError::InvalidArguments);
    }

    let iovs = if msg.msg_iovlen == 0 {
        &[][..]
    } else {
        if msg.msg_iov.is_null() {
            return Err(SyscallError::BadAddress);
        }
        unsafe { core::slice::from_raw_parts(msg.msg_iov, msg.msg_iovlen) }
    };

    if !msg.msg_name.is_null() {
        let address_len = msg.msg_namelen;
        let path = path_from_sockaddr(msg.msg_name.cast(), address_len)?;
        let socket_ref = socket.clone().as_unix_socket()?;
        if matches!(
            &*socket_ref.state.lock(),
            crate::socket::UnixSocketState::Unbound
        ) {
            socket_ref
                .connect(path)
                .map_err(crate::object::error::ObjectError::from)?;
        }
    }

    let socket = socket.as_unix_socket()?;
    let mut total_written = 0usize;

    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let buffer = unsafe { core::slice::from_raw_parts(iov.iov_base.cast_const(), iov.iov_len) };
        let written = socket
            .write_socket(buffer)
            .map_err(crate::object::error::ObjectError::from)?;
        total_written += written;
        if written < buffer.len() {
            break;
        }
    }

    Ok(total_written)
});

define_syscall!(Setsockopt, |socket: ObjectRef,
                             level: i32,
                             option_name: i32,
                             option_value: *const u8,
                             option_len: u32| {
    if option_len > 0 && option_value.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let option_value = unsafe { slice::from_raw_parts(option_value, option_len as usize) };
    socket
        .as_unix_socket()?
        .setsockopt(level as u64, option_name as u64, option_value)
        .map_err(crate::object::error::ObjectError::from)?;

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

        let option_len = unsafe { *option_len_ptr as usize };
        let value = socket
            .as_unix_socket()?
            .getsockopt(level as u64, option_name as u64, option_len)
            .map_err(crate::object::error::ObjectError::from)?;

        if !value.is_empty() && option_value.is_null() {
            return Err(SyscallError::BadAddress);
        }

        if !value.is_empty() {
            user_safe::write(option_value, &value[..])?;
        }
        user_safe::write(option_len_ptr, &(value.len() as u32))?;

        Ok(0)
    }
);

define_syscall!(
    Getsockname,
    |socket: ObjectRef, address: *mut u8, address_len_ptr: *mut u32| {
        if address_len_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let name = socket
            .as_unix_socket()?
            .getsockname_bytes()
            .map_err(crate::object::error::ObjectError::from)?;
        let requested_len = unsafe { *address_len_ptr as usize };
        let copy_len = requested_len.min(name.len());

        if copy_len > 0 && address.is_null() {
            return Err(SyscallError::BadAddress);
        }

        if copy_len > 0 {
            user_safe::write(address, &name[..copy_len])?;
        }
        user_safe::write(address_len_ptr, &(name.len() as u32))?;

        Ok(0)
    }
);

define_syscall!(
    Getpeername,
    |socket: ObjectRef, address: *mut u8, address_len_ptr: *mut u32| {
        if address_len_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let name = socket
            .as_unix_socket()?
            .getpeername_bytes()
            .map_err(crate::object::error::ObjectError::from)?;
        let requested_len = unsafe { *address_len_ptr as usize };
        let copy_len = requested_len.min(name.len());

        if copy_len > 0 && address.is_null() {
            return Err(SyscallError::BadAddress);
        }

        if copy_len > 0 {
            user_safe::write(address, &name[..copy_len])?;
        }
        user_safe::write(address_len_ptr, &(name.len() as u32))?;

        Ok(0)
    }
);

define_syscall!(Recvmsg, |socket: ObjectRef,
                          msg: *mut relibc_msg_hdr,
                          _flags: u64| {
    if msg.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let msg = unsafe { &mut *msg };
    if msg.msg_iovlen > isize::MAX as usize {
        return Err(SyscallError::InvalidArguments);
    }

    let iovs = if msg.msg_iovlen == 0 {
        &[][..]
    } else {
        if msg.msg_iov.is_null() {
            return Err(SyscallError::BadAddress);
        }
        unsafe { core::slice::from_raw_parts_mut(msg.msg_iov, msg.msg_iovlen) }
    };

    let socket = socket.as_unix_socket()?;
    let mut total_read = 0usize;

    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let buffer = unsafe { core::slice::from_raw_parts_mut(iov.iov_base, iov.iov_len) };
        let read = socket
            .read_socket(buffer)
            .map_err(crate::object::error::ObjectError::from)?;
        total_read += read;
        if read < buffer.len() {
            break;
        }
    }

    msg.msg_flags = 0;
    if !msg.msg_name.is_null() {
        msg.msg_namelen = 0;
    }
    msg.msg_controllen = 0;

    Ok(total_read)
});

define_syscall!(Shutdown, |socket: ObjectRef, how: u64| {
    socket
        .as_unix_socket()?
        .shutdown(how)
        .map_err(crate::object::error::ObjectError::from)?;
    Ok(0)
});

#[repr(C)]
struct relibc_iovec {
    iov_base: *mut u8,
    iov_len: usize,
}

#[repr(C)]
struct relibc_msg_hdr {
    msg_name: *mut u8,
    msg_namelen: u32,
    msg_iov: *mut relibc_iovec,
    msg_iovlen: usize,
    msg_control: *mut u8,
    msg_controllen: usize,
    msg_flags: i32,
}
