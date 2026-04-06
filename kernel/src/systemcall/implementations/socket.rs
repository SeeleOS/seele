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
