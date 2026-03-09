use alloc::sync::Arc;

use crate::{
    multitasking::MANAGER,
    object::{Object, error::ObjectError},
};

pub type ObjectRef = Arc<dyn Object>;
pub type ObjectResult<T> = Result<T, ObjectError>;

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
