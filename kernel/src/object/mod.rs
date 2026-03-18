use core::fmt::Debug;

use alloc::sync::Arc;

use crate::{
    filesystem::{info::LinuxStat, object::FileLikeObject},
    multitasking::MANAGER,
    object::{
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, Readable, Writable},
    },
};

pub mod config;
pub mod error;
pub mod misc;
pub mod traits;
pub mod tty_device;
macro_rules! define_cast_function_non_trait {
    ($name: expr, $type: ty) => {
        paste::paste! {
            fn [<as_$name>](self: Arc<Self>) -> Option<Arc<$type>> {
                None
            }
        }
    };
}

macro_rules! define_cast_function {
    ($name: expr, $type: ty) => {
        paste::paste! {
            fn [<as_$name>](self: Arc<Self>) -> Option<Arc<dyn $type>> {
                None
            }
        }
    };
}

pub trait Object: Send + Sync + Debug {
    define_cast_function!(writable, Writable);
    define_cast_function!(readable, Readable);
    define_cast_function!(configuratable, Configuratable);

    define_cast_function_non_trait!(file_like, FileLikeObject);
}
