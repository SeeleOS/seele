use crate::{
    memory::user_safe,
    net::InetAddress,
    object::netlink::{NetlinkSocketAddress, NetlinkSocketObject},
    object::{
        FileFlags,
        error::ObjectError,
        misc::{ObjectRef, get_object_current_process},
    },
    process::{FdFlags, manager::get_current_process},
    socket::{
        AF_INET, AF_NETLINK, AF_UNIX, SOCK_CLOEXEC, SOCK_NONBLOCK, SOL_SOCKET, SocketLike,
        UnixSocketKind, UnixSocketObject, UnixSocketState,
    },
    systemcall::utils::SyscallError,
};
use alloc::{string::String, vec, vec::Vec};
use core::{mem, slice};

mod address;
mod control;
mod syscalls;
mod types;

use address::*;
use control::*;
use types::*;

pub use syscalls::*;

fn socket_like(socket: ObjectRef) -> Result<alloc::sync::Arc<dyn SocketLike>, SyscallError> {
    if socket
        .clone()
        .get_flags()
        .is_ok_and(|flags| flags.contains(FileFlags::PATH))
    {
        return Err(SyscallError::BadFileDescriptor);
    }
    socket.as_socket_like().map_err(|_| SyscallError::NotSocket)
}
