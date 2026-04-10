use alloc::{string::String, vec};

use crate::{
    define_syscall,
    misc::usercopy::{copy_from_user, copy_to_user, read_user_value, write_user_value},
    object::misc::ObjectRef,
    process::manager::get_current_process,
    systemcall::utils::{SyscallError, SyscallImpl},
};

define_syscall!(Socket, |domain: u64, kind: u64, protocol: u64| {
    let socket = crate::socket::UnixSocketObject::create(domain, kind, protocol)
        .map_err(crate::object::error::ObjectError::from)?;
    let fd = get_current_process().lock().push_object(socket);
    Ok(fd)
});

define_syscall!(SocketBind, |socket: ObjectRef, path: String| {
    socket
        .as_unix_socket()?
        .bind(path)
        .map_err(crate::object::error::ObjectError::from)?;
    Ok(0)
});

define_syscall!(SocketListen, |socket: ObjectRef, backlog: usize| {
    socket
        .as_unix_socket()?
        .listen(backlog)
        .map_err(crate::object::error::ObjectError::from)?;
    Ok(0)
});

define_syscall!(SocketConnect, |socket: ObjectRef, path: String| {
    socket
        .as_unix_socket()?
        .connect(path)
        .map_err(crate::object::error::ObjectError::from)?;
    Ok(0)
});

define_syscall!(SocketAccept, |socket: ObjectRef| {
    Ok(socket
        .as_unix_socket()?
        .accept()
        .map_err(crate::object::error::ObjectError::from)?)
});

define_syscall!(
    SocketGetSockOpt,
    |socket: ObjectRef,
     level: i32,
     option_name: i32,
     option_value: *mut u8,
     option_len_ptr: *mut u32| {
        if option_len_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let option_len =
            read_user_value(option_len_ptr as *const u32).ok_or(SyscallError::BadAddress)? as usize;
        let value = socket
            .as_unix_socket()?
            .getsockopt(level as u64, option_name as u64, option_len)
            .map_err(crate::object::error::ObjectError::from)?;

        if !value.is_empty() && option_value.is_null() {
            return Err(SyscallError::BadAddress);
        }

        if !value.is_empty() && !copy_to_user(option_value, &value) {
            return Err(SyscallError::BadAddress);
        }
        if !write_user_value(option_len_ptr, value.len() as u32) {
            return Err(SyscallError::BadAddress);
        }

        Ok(0)
    }
);

define_syscall!(
    SocketGetSockName,
    |socket: ObjectRef, address: *mut u8, address_len_ptr: *mut u32| {
        if address_len_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let name = socket
            .as_unix_socket()?
            .getsockname_bytes()
            .map_err(crate::object::error::ObjectError::from)?;
        let requested_len = read_user_value(address_len_ptr as *const u32)
            .ok_or(SyscallError::BadAddress)? as usize;
        let copy_len = requested_len.min(name.len());

        if copy_len > 0 && address.is_null() {
            return Err(SyscallError::BadAddress);
        }

        if copy_len > 0 && !copy_to_user(address, &name[..copy_len]) {
            return Err(SyscallError::BadAddress);
        }
        if !write_user_value(address_len_ptr, name.len() as u32) {
            return Err(SyscallError::BadAddress);
        }

        Ok(0)
    }
);

define_syscall!(
    SocketGetPeerName,
    |socket: ObjectRef, address: *mut u8, address_len_ptr: *mut u32| {
        if address_len_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let name = socket
            .as_unix_socket()?
            .getpeername_bytes()
            .map_err(crate::object::error::ObjectError::from)?;
        let requested_len = read_user_value(address_len_ptr as *const u32)
            .ok_or(SyscallError::BadAddress)? as usize;
        let copy_len = requested_len.min(name.len());

        if copy_len > 0 && address.is_null() {
            return Err(SyscallError::BadAddress);
        }

        if copy_len > 0 && !copy_to_user(address, &name[..copy_len]) {
            return Err(SyscallError::BadAddress);
        }
        if !write_user_value(address_len_ptr, name.len() as u32) {
            return Err(SyscallError::BadAddress);
        }

        Ok(0)
    }
);

define_syscall!(SocketRecvMsg, |socket: ObjectRef, msg_ptr: *mut u8, _flags: u64| {
    if msg_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut msg =
        read_user_value(msg_ptr as *const relibc_msg_hdr).ok_or(SyscallError::BadAddress)?;
    if msg.msg_iovlen > isize::MAX as usize {
        return Err(SyscallError::InvalidArguments);
    }

    let iovs = if msg.msg_iovlen == 0 {
        vec![]
    } else {
        if msg.msg_iov.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let mut iovs = vec![relibc_iovec::default(); msg.msg_iovlen];
        let iov_bytes = unsafe {
            core::slice::from_raw_parts_mut(
                iovs.as_mut_ptr().cast::<u8>(),
                msg.msg_iovlen * core::mem::size_of::<relibc_iovec>(),
            )
        };
        if !copy_from_user(msg.msg_iov.cast::<u8>(), iov_bytes) {
            return Err(SyscallError::BadAddress);
        }
        iovs
    };

    let socket = socket.as_unix_socket()?;
    let mut total_read = 0usize;

    for iov in &iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let mut buffer = vec![0u8; iov.iov_len];
        let read = socket.read_socket(&mut buffer).map_err(crate::object::error::ObjectError::from)?;
        if !copy_to_user(iov.iov_base, &buffer[..read]) {
            return Err(SyscallError::BadAddress);
        }
        total_read += read;
        if read < iov.iov_len {
            break;
        }
    }

    msg.msg_flags = 0;
    if !msg.msg_name.is_null() {
        msg.msg_namelen = 0;
    }
    msg.msg_controllen = 0;
    if !write_user_value(msg_ptr as *mut relibc_msg_hdr, msg) {
        return Err(SyscallError::BadAddress);
    }

    Ok(total_read)
});

define_syscall!(SocketShutdown, |socket: ObjectRef, how: u64| {
    socket
        .as_unix_socket()?
        .shutdown(how)
        .map_err(crate::object::error::ObjectError::from)?;
    Ok(0)
});

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct relibc_iovec {
    iov_base: *mut u8,
    iov_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct relibc_msg_hdr {
    msg_name: *mut u8,
    msg_namelen: u32,
    msg_iov: *mut relibc_iovec,
    msg_iovlen: usize,
    msg_control: *mut u8,
    msg_controllen: usize,
    msg_flags: i32,
}
