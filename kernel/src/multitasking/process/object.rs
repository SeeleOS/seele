use alloc::{collections::vec_deque::VecDeque, sync::Arc, vec::Vec};

use crate::{
    multitasking::process::Process,
    object::{
        Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        tty_device::get_default_tty,
    },
};

pub fn init_objects(objects: &mut Vec<Option<Arc<dyn Object>>>) {
    objects.push(Some(get_default_tty())); // stdin (unimpllemented)
    objects.push(Some(get_default_tty())); // stdout
    objects.push(Some(get_default_tty())); // stderr
}

impl Process {
    pub fn get_object(&mut self, index: u64) -> ObjectResult<ObjectRef> {
        self.objects
            .get(index as usize)
            .ok_or(ObjectError::DoesNotExist)?
            .clone()
            .ok_or(ObjectError::DoesNotExist)
    }
}
