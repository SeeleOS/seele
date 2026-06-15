use crate::{
    define_syscall,
    memory::user_safe,
    object::netlink::NetlinkSocketObject,
    object::{
        FileFlags,
        error::ObjectError,
        misc::{ObjectRef, get_object_current_process},
    },
    process::{FdFlags, manager::get_current_process},
    socket::{
        AF_INET, AF_NETLINK, InetSocketObject, SOCK_CLOEXEC, SOCK_NONBLOCK, SOL_SOCKET,
        UnixSocketKind, UnixSocketState,
    },
    systemcall::utils::{SyscallError, SyscallImpl},
};
use alloc::{vec, vec::Vec};
use core::{mem, slice};

use super::*;

mod connection;
mod sendrecv;
mod sockopt;

pub use connection::*;
pub use sendrecv::*;
pub use sockopt::*;
