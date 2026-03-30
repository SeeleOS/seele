use core::fmt::Debug;

use alloc::sync::Arc;

use crate::{
    filesystem::object::FileLikeObject,
    object::{
        misc::ObjectResult,
        traits::{Configuratable, Controllable, Readable, Writable},
    },
    polling::{object::Pollable, poller::PollerObject},
};

pub mod config;
pub mod control;
pub mod error;
pub mod misc;
pub mod traits;
pub mod tty_device;

macro_rules! define_cast_function_non_trait {
    ($name: expr, $type: ty, $err: ty) => {
        paste::paste! {
            fn [<as_$name>](self: Arc<Self>) -> Result<Arc<$type>, $crate::systemcall::error::SyscallError> {
                Err($crate::systemcall::error::SyscallError::$err)
            }
        }
    };
}

macro_rules! define_cast_function {
    ($name: expr, $type: ty, $err: ty) => {
        paste::paste! {
            fn [<as_$name>](self: Arc<Self>) -> Result<Arc<dyn $type>, $crate::systemcall::error::SyscallError> {
                Err($crate::systemcall::error::SyscallError::$err)
            }
        }
    };
}

pub trait Object: Send + Sync + Debug {
    define_cast_function!(writable, Writable, BadFileDescriptor);
    define_cast_function!(readable, Readable, BadFileDescriptor);
    define_cast_function!(configuratable, Configuratable, InappropriateIoctl);
    define_cast_function!(controllable, Controllable, InvalidArguments);
    define_cast_function!(pollable, Pollable, InvalidArguments);

    define_cast_function_non_trait!(file_like, FileLikeObject, BadFileDescriptor);
    define_cast_function_non_trait!(poller, PollerObject, BadFileDescriptor);
}
