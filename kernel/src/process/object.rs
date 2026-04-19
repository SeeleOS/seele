use alloc::{sync::Arc, vec::Vec};

use crate::{
    object::{
        Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        tty_device::get_default_tty,
    },
    process::{FdFlags, Process},
};

pub fn init_objects(objects: &mut Vec<Option<Arc<dyn Object>>>, object_flags: &mut Vec<FdFlags>) {
    if objects.len() < 3 {
        objects.resize(3, None);
    }
    if object_flags.len() < objects.len() {
        object_flags.resize(objects.len(), FdFlags::empty());
    }

    for slot in 0..3 {
        if objects[slot].is_none() {
            objects[slot] = Some(get_default_tty());
            object_flags[slot] = FdFlags::empty();
        }
    }
}

impl Process {
    fn ensure_object_flags_len(&mut self) {
        if self.object_flags.len() < self.objects.len() {
            self.object_flags
                .resize(self.objects.len(), FdFlags::empty());
        }
    }

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
            self.object_flags.push(FdFlags::empty());
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
            self.object_flags.resize(min + 1, FdFlags::empty());
            min
        } else {
            self.objects.push(None);
            self.object_flags.push(FdFlags::empty());
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

    pub fn set_object_slot_with_flags(
        &mut self,
        slot: usize,
        object: ObjectRef,
        flags: FdFlags,
    ) -> ObjectResult<usize> {
        if self.objects.len() <= slot {
            self.objects.resize(slot + 1, None);
        }
        self.ensure_object_flags_len();
        if self.object_flags.len() <= slot {
            self.object_flags.resize(slot + 1, FdFlags::empty());
        }
        self.objects[slot] = Some(object);
        self.object_flags[slot] = flags;
        Ok(slot)
    }

    pub fn clear_object_slot(&mut self, slot: usize) -> ObjectResult<()> {
        let entry = self
            .objects
            .get_mut(slot)
            .ok_or(ObjectError::DoesNotExist)?;
        if entry.is_none() {
            return Err(ObjectError::DoesNotExist);
        }
        *entry = None;
        if let Some(flags) = self.object_flags.get_mut(slot) {
            *flags = FdFlags::empty();
        }
        Ok(())
    }

    pub fn get_fd_flags(&self, index: usize) -> ObjectResult<FdFlags> {
        let object = self.objects.get(index).ok_or(ObjectError::DoesNotExist)?;
        if object.is_none() {
            return Err(ObjectError::DoesNotExist);
        }
        Ok(self
            .object_flags
            .get(index)
            .copied()
            .unwrap_or_else(FdFlags::empty))
    }

    pub fn set_fd_flags(&mut self, index: usize, flags: FdFlags) -> ObjectResult<()> {
        let object = self.objects.get(index).ok_or(ObjectError::DoesNotExist)?;
        if object.is_none() {
            return Err(ObjectError::DoesNotExist);
        }
        self.ensure_object_flags_len();
        self.object_flags[index] = flags;
        Ok(())
    }

    pub fn clone_object(&mut self, object: ObjectRef) -> ObjectResult<usize> {
        let slot = self.alloc_object_slot();
        self.set_object_slot_with_flags(slot, object.clone(), FdFlags::empty())
    }

    pub fn clone_object_with_min(&mut self, object: ObjectRef, min: usize) -> ObjectResult<usize> {
        self.clone_object_with_min_and_flags(object, min, FdFlags::empty())
    }

    pub fn clone_object_with_min_and_flags(
        &mut self,
        object: ObjectRef,
        min: usize,
        flags: FdFlags,
    ) -> ObjectResult<usize> {
        let slot = self.alloc_object_slot_with_min(min);
        self.set_object_slot_with_flags(slot, object.clone(), flags)
    }

    pub fn clone_object_to(&mut self, object: ObjectRef, dest: usize) -> ObjectResult<usize> {
        self.clone_object_to_with_flags(object, dest, FdFlags::empty())
    }

    pub fn clone_object_to_with_flags(
        &mut self,
        object: ObjectRef,
        dest: usize,
        flags: FdFlags,
    ) -> ObjectResult<usize> {
        self.set_object_slot_with_flags(dest, object.clone(), flags)
    }

    pub fn push_object(&mut self, object: ObjectRef) -> usize {
        self.push_object_with_flags(object, FdFlags::empty())
    }

    pub fn push_object_with_flags(&mut self, object: ObjectRef, flags: FdFlags) -> usize {
        let slot = self.alloc_object_slot();
        let _ = self.set_object_slot_with_flags(slot, object, flags);
        slot
    }

    pub fn close_cloexec_objects(&mut self) {
        self.ensure_object_flags_len();
        for (slot, object) in self.objects.iter_mut().enumerate() {
            if object.is_some() && self.object_flags[slot].contains(FdFlags::CLOEXEC) {
                *object = None;
                self.object_flags[slot] = FdFlags::empty();
            }
        }
    }
}
