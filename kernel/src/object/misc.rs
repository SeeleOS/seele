use alloc::sync::Arc;

use crate::object::Object;

pub type ObjectRef = Arc<dyn Object>;
