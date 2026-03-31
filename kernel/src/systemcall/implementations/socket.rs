use alloc::string::String;

use crate::{
    define_syscall,
    object::misc::ObjectRef,
    process::manager::get_current_process,
    systemcall::utils::SyscallImpl,
};

define_syscall!(Socket, |domain: u64, kind: u64, protocol: u64| {
    let socket = crate::misc::socket::UnixSocketObject::create(domain, kind, protocol)?;
    let fd = get_current_process().lock().push_object(socket);
    Ok(fd)
});

define_syscall!(SocketBind, |socket: ObjectRef, path: String| {
    socket.as_unix_socket()?.bind(path)?;
    Ok(0)
});

define_syscall!(SocketListen, |socket: ObjectRef, backlog: usize| {
    socket.as_unix_socket()?.listen(backlog)?;
    Ok(0)
});

define_syscall!(SocketConnect, |socket: ObjectRef, path: String| {
    socket.as_unix_socket()?.connect(path)?;
    Ok(0)
});

define_syscall!(SocketAccept, |socket: ObjectRef| {
    Ok(socket.as_unix_socket()?.accept()?)
});
