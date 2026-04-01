use alloc::string::String;

use crate::{
    define_syscall,
    object::misc::ObjectRef,
    process::manager::get_current_process,
    systemcall::utils::SyscallImpl,
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
