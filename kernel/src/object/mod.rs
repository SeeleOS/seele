use core::fmt::Debug;

use alloc::sync::Arc;
use seele_sys::{SyscallResult, abi::object::ObjectFlags, errors::SyscallError};

use crate::{
    filesystem::object::FileLikeObject,
    object::{
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, MemoryMappable, Readable, Seekable, Statable, Writable},
    },
    polling::{object::Pollable, poller::PollerObject},
    socket::UnixSocketObject,
};

pub mod config;
pub mod control;
pub mod device;
pub mod error;
pub mod misc;
pub mod queue_helpers;
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
    fn debug_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }

    fn get_flags(self: Arc<Self>) -> ObjectResult<ObjectFlags> {
        Err(ObjectError::Unimplemented)
    }

    fn set_flags(self: Arc<Self>, flags: ObjectFlags) -> ObjectResult<()> {
        Err(ObjectError::Unimplemented)
    }

    define_cast_function!("writable", Writable, BadFileDescriptor);
    define_cast_function!("readable", Readable, BadFileDescriptor);
    define_cast_function!("configuratable", Configuratable, InappropriateIoctl);
    define_cast_function!("pollable", Pollable, InvalidArguments);
    define_cast_function!("mappable", MemoryMappable, InvalidArguments);
    define_cast_function!("seekable", Seekable, InvalidArguments);
    define_cast_function!("statable", Statable, BadFileDescriptor);

    define_cast_function_non_trait!("file_like", FileLikeObject, BadFileDescriptor);
    define_cast_function_non_trait!("poller", PollerObject, BadFileDescriptor);
    define_cast_function_non_trait!("unix_socket", UnixSocketObject, BadFileDescriptor);
}
