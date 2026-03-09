use core::fmt::Debug;

use alloc::sync::Arc;

use crate::{
    filesystem::info::LinuxStat,
    multitasking::MANAGER,
    object::{config::Configuratable, error::ObjectError, misc::ObjectResult},
};

pub mod config;
pub mod error;
pub mod misc;
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
macro_rules! is_writable {
    () => {
        fn as_writable(self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn Writable>> {
            Some(self)
        }
    };
}
#[macro_export]
macro_rules! is_readable {
    () => {
        fn as_readable(self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn Readable>> {
            Some(self)
        }
    };
}

#[macro_export]
macro_rules! have_linux_stat {
    () => {
        fn as_have_linux_stat(
            self: alloc::sync::Arc<Self>,
        ) -> Option<alloc::sync::Arc<dyn HaveLinuxStat>> {
            Some(self)
        }
    };
}

pub trait Writable: Object {
    /// Write the content of [`buffer`] to [`self`]
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize>;
}

pub trait Readable: Object {
    /// Reads the content of [`self`] and write them to [`buffer`]
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize>;
}

pub trait HaveLinuxStat: Object {
    fn stat(&self) -> ObjectResult<LinuxStat>;
}

pub fn get_object(id: u64) -> Option<Arc<dyn Object>> {
    let current = MANAGER.lock().current.clone().unwrap();
    let current = current.lock();

    current.objects.get(id as usize).cloned()?
}
