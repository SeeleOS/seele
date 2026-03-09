use core::fmt::Debug;

use alloc::sync::Arc;

use crate::{
    filesystem::info::LinuxStat,
    multitasking::MANAGER,
    object::{
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, HaveLinuxStat, Readable, Writable},
    },
};

pub mod config;
pub mod error;
pub mod misc;
pub mod traits;
pub mod tty_device;

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
    define_cast_function!(have_linux_stat, HaveLinuxStat);
}

#[macro_export]
macro_rules! impl_cast_function {
    ($fn_name: expr, $type:ty) => {
        paste::paste! {
        fn [<as_$fn_name>](self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn $type>> {
            Some(self)
        }
        }
    };
}

pub fn get_object(id: u64) -> Option<Arc<dyn Object>> {
    let current = MANAGER.lock().current.clone().unwrap();
    let current = current.lock();

    current.objects.get(id as usize).cloned()?
}
