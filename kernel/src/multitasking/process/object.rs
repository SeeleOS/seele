use alloc::{sync::Arc, vec::Vec};

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
    // Allocates a slot on the objects vec
    pub fn alloc_object_slot(&mut self) -> usize {
        if let Some((i, _)) = self.objects.iter().enumerate().find(|(_, p)| p.is_none()) {
            i
        } else {
            self.objects.push(None);
            self.objects.len() - 1
        }
    }
    pub fn get_object(&mut self, index: u64) -> ObjectResult<ObjectRef> {
        self.objects
            .get(index as usize)
            .ok_or(ObjectError::DoesNotExist)?
            .clone()
            .ok_or(ObjectError::DoesNotExist)
    }

    pub fn clone_object(&mut self, object: ObjectRef) -> ObjectResult<usize> {
        let slot = self.alloc_object_slot();
        self.objects[slot] = Some(object.clone());
        Ok(slot)
    }
}
