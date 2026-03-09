use alloc::sync::Arc;

use crate::object::{Object, error::ObjectError};

pub type ObjectRef = Arc<dyn Object>;
pub type ObjectResult<T> = Result<T, ObjectError>;
