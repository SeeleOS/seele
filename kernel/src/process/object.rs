use alloc::{sync::Arc, vec::Vec};

use crate::{
    object::{
        Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        tty_device::get_default_tty,
    },
    process::Process,
};

pub fn init_objects(objects: &mut Vec<Option<Arc<dyn Object>>>) {
    objects.push(Some(get_default_tty())); // stdin (unimpllemented)
    objects.push(Some(get_default_tty())); // stdout
    objects.push(Some(get_default_tty())); // stderr
}

impl Process {
    pub fn find_empty_object_slot(&self, starts_from: usize) -> Option<usize> {
        self.objects
            .iter()
            .enumerate()
            .skip(starts_from)
            .find(|(_, p)| p.is_none())
            .map(|(i, _)| i)
    }

    // Allocates a slot on the objects vec
    pub fn alloc_object_slot(&mut self) -> usize {
        if let Some(i) = self.find_empty_object_slot(0) {
            i
        } else {
            self.objects.push(None);
            self.objects.len() - 1
        }
    }

    /// Same as alloc_object_slot, but with a minimum index requirement
    pub fn alloc_object_slot_with_min(&mut self, min: usize) -> usize {
        if let Some(i) = self.find_empty_object_slot(min) {
            return i;
        }

        if self.objects.len() <= min {
            self.objects.resize(min + 1, None);
            min
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

    pub fn clone_object_to(&mut self, object: ObjectRef, dest: usize) -> ObjectResult<usize> {
        self.objects.resize(dest + 1, None);
        self.objects[dest] = Some(object.clone());
        Ok(dest)
    }

    pub fn push_object(&mut self, object: ObjectRef) -> usize {
        let slot = self.alloc_object_slot();

        self.objects[slot] = Some(object);

        slot
    }
}
