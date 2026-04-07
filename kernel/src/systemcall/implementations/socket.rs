use alloc::string::String;
use core::ptr;

use crate::{
    define_syscall,
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

        let option_len = unsafe { *option_len_ptr as usize };
        let value = socket
            .as_unix_socket()?
            .getsockopt(level as u64, option_name as u64, option_len)
            .map_err(crate::object::error::ObjectError::from)?;

        if !value.is_empty() && option_value.is_null() {
            return Err(SyscallError::BadAddress);
        }

        unsafe {
            if !value.is_empty() {
                ptr::copy_nonoverlapping(value.as_ptr(), option_value, value.len());
            }
            *option_len_ptr = value.len() as u32;
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
        let requested_len = unsafe { *address_len_ptr as usize };
        let copy_len = requested_len.min(name.len());

        if copy_len > 0 && address.is_null() {
            return Err(SyscallError::BadAddress);
        }

        unsafe {
            if copy_len > 0 {
                ptr::copy_nonoverlapping(name.as_ptr(), address, copy_len);
            }
            *address_len_ptr = name.len() as u32;
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
        let requested_len = unsafe { *address_len_ptr as usize };
        let copy_len = requested_len.min(name.len());

        if copy_len > 0 && address.is_null() {
            return Err(SyscallError::BadAddress);
        }

        unsafe {
            if copy_len > 0 {
                ptr::copy_nonoverlapping(name.as_ptr(), address, copy_len);
            }
            *address_len_ptr = name.len() as u32;
        }

        Ok(0)
    }
);

define_syscall!(SocketRecvMsg, |socket: ObjectRef, msg_ptr: *mut u8, _flags: u64| {
    if msg_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let msg = unsafe { &mut *(msg_ptr as *mut relibc_msg_hdr) };
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

define_syscall!(SocketShutdown, |socket: ObjectRef, how: u64| {
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
