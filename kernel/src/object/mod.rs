use core::fmt::Debug;

use alloc::sync::Arc;
use seele_sys::{SyscallResult, errors::SyscallError};

use crate::{
    filesystem::object::FileLikeObject,
    misc::socket::UnixSocketObject,
    object::{
        misc::ObjectResult,
        traits::{Configuratable, Controllable, MemoryMappable, Readable, Writable},
    },
    polling::{object::Pollable, poller::PollerObject},
};

pub mod config;
pub mod control;
pub mod device;
pub mod error;
pub mod misc;
pub mod traits;
pub mod tty_device;

macro_rules! define_cast_function_non_trait {
    ($name: literal, $type: ty, $err: ty) => {
        paste::paste! {
            fn [<as_$name>](self: Arc<Self>) -> SyscallResult<Arc<$type>> {
                Err(SyscallError::$err)
            }
        }
    };
}

macro_rules! define_cast_function {
    ($name: literal, $type: ty, $err: ty) => {
        paste::paste! {
            fn [<as_$name>](self: Arc<Self>) -> SyscallResult<Arc<dyn $type>> {
                Err(SyscallError::$err)
            }
        }
    };
}

pub trait Object: Send + Sync + Debug {
    define_cast_function!("writable", Writable, BadFileDescriptor);
    define_cast_function!("readable", Readable, BadFileDescriptor);
    define_cast_function!("configuratable", Configuratable, InappropriateIoctl);
    define_cast_function!("controllable", Controllable, InvalidArguments);
    define_cast_function!("pollable", Pollable, InvalidArguments);
    define_cast_function!("mappable", MemoryMappable, InvalidArguments);

    define_cast_function_non_trait!("file_like", FileLikeObject, BadFileDescriptor);
    define_cast_function_non_trait!("poller", PollerObject, BadFileDescriptor);
    define_cast_function_non_trait!("unix_socket", UnixSocketObject, BadFileDescriptor);
}
