use alloc::sync::Arc;

use crate::{
    object::{Object, error::ObjectError},
    process::manager::get_current_process,
};

pub type ObjectRef = Arc<dyn Object>;
pub type ObjectResult<T> = Result<T, ObjectError>;

#[macro_export]
macro_rules! impl_cast_function_non_trait {
    ($fn_name: literal, $type:ty) => {
        paste::paste! {
        fn [<as_$fn_name>](self: alloc::sync::Arc<Self>) -> $crate::systemcall::utils::SyscallResult<alloc::sync::Arc<$type>> {
            Ok(self)
        }
        }
    };
}

#[macro_export]
macro_rules! impl_cast_function {
    ($fn_name: literal, $type:ty) => {
        paste::paste! {
        fn [<as_$fn_name>](self: alloc::sync::Arc<Self>) -> $crate::systemcall::utils::SyscallResult<alloc::sync::Arc<dyn $type>> {
            Ok(self)
        }
        }
    };
}

pub fn get_object_current_process(id: u64) -> ObjectResult<Arc<dyn Object>> {
    let current = get_current_process();
    let current = current.lock();

    current
        .fd_table
        .get(id as usize)
        .and_then(|entry| entry.as_ref())
        .map(|entry| entry.object.clone())
        .ok_or(ObjectError::DoesNotExist)
}
