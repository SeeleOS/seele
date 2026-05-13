use alloc::sync::{Arc, Weak};

use crate::memory::utils::Mut;

pub(crate) fn object_ref<T>(self_ref: &Mut<Option<Weak<T>>>) -> Option<Arc<T>> {
    self_ref.lock().as_ref().and_then(Weak::upgrade)
}
