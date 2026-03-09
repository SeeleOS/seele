use core::fmt::Debug;

use alloc::sync::Arc;

use crate::{
    multitasking::MANAGER,
    object::{config::Configuratable, error::ObjectError},
};

pub mod config;
pub mod error;
pub mod tty_device;

pub trait Object: Send + Sync + Debug {
    fn as_writable(self: Arc<Self>) -> Option<Arc<dyn Writable>> {
        None
    }

    fn as_readable(self: Arc<Self>) -> Option<Arc<dyn Readable>> {
        None
    }

    fn as_configuratable(self: Arc<Self>) -> Option<Arc<dyn Configuratable>> {
        None
    }
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

pub type ObjectResult<T> = Result<T, ObjectError>;

pub trait Writable: Object {
    /// Write the content of [`buffer`] to [`self`]
    fn write(&mut self, buffer: &[u8]) -> ObjectResult<usize>;
}

pub trait Readable: Object {
    /// Reads the content of [`self`] and write them to [`buffer`]
    fn read(&mut self, buffer: &mut [u8]) -> ObjectResult<usize>;
}

pub fn get_object(id: u64) -> Option<Arc<dyn Object>> {
    let current = MANAGER.lock().current.clone().unwrap();
    let current = current.lock();

    current.objects.get(id as usize).cloned()
}
