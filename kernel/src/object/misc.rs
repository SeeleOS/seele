use alloc::sync::Arc;

use crate::{
    multitasking::MANAGER,
    object::{Object, error::ObjectError},
};

pub type ObjectRef = Arc<dyn Object>;
pub type ObjectResult<T> = Result<T, ObjectError>;
#[macro_export]
macro_rules! impl_cast_function_non_trait {
    ($fn_name: expr, $type:ty) => {
        paste::paste! {
        fn [<as_$fn_name>](self: alloc::sync::Arc<Self>) -> Result<alloc::sync::Arc<$type>, $crate::systemcall::error::SyscallError> {
            Ok(self)
        }
        }
    };
}

#[macro_export]
macro_rules! impl_cast_function {
    ($fn_name: expr, $type:ty) => {
        paste::paste! {
        fn [<as_$fn_name>](self: alloc::sync::Arc<Self>) -> Result<alloc::sync::Arc<dyn $type>, $crate::systemcall::error::SyscallError> {
            Ok(self)
        }
        }
    };
}

pub fn get_object_current_process(id: u64) -> ObjectResult<Arc<dyn Object>> {
    let current = MANAGER.lock().current.clone().unwrap();
    let current = current.lock();

    current
        .objects
        .get(id as usize)
        .cloned()
        .ok_or(ObjectError::DoesNotExist)?
        .ok_or(ObjectError::DoesNotExist)
}
