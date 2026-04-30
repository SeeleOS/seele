use alloc::{sync::Arc, vec::Vec};

use crate::{
    object::{
        error::ObjectError,
        file_locks::{release_fd_entry_locks, release_process_fd_table_locks},
        misc::{ObjectRef, ObjectResult},
        tty_device::get_default_tty,
    },
    process::{FdEntry, FdFlags, Process, new_fd_table},
};

pub fn close_cloexec_fd_entries(fd_table: &mut [Option<FdEntry>]) {
    for entry in fd_table {
        if entry
            .as_ref()
            .is_some_and(|entry| entry.fd_flags.contains(FdFlags::CLOEXEC))
        {
            *entry = None;
        }
    }
}

pub fn init_objects(fd_table: &mut Vec<Option<FdEntry>>) {
    if fd_table.len() < 3 {
        fd_table.resize(3, None);
    }

    for entry in fd_table.iter_mut().take(3) {
        if entry.is_none() {
            *entry = Some(FdEntry::new(get_default_tty(), FdFlags::empty()));
        }
    }
}

impl Process {
    pub fn find_empty_fd_slot(&self, starts_from: usize) -> Option<usize> {
        self.fd_table
            .lock()
            .iter()
            .enumerate()
            .skip(starts_from)
            .find(|(_, entry)| entry.is_none())
            .map(|(index, _)| index)
    }

    pub fn alloc_fd_slot(&mut self) -> usize {
        if let Some(index) = self.find_empty_fd_slot(0) {
            index
        } else {
            let mut fd_table = self.fd_table.lock();
            fd_table.push(None);
            fd_table.len() - 1
        }
    }

    pub fn alloc_fd_slot_with_min(&mut self, min: usize) -> usize {
        if let Some(index) = self.find_empty_fd_slot(min) {
            return index;
        }

        let mut fd_table = self.fd_table.lock();
        if fd_table.len() <= min {
            fd_table.resize(min + 1, None);
            min
        } else {
            fd_table.push(None);
            fd_table.len() - 1
        }
    }

    pub fn get_object(&self, index: u64) -> ObjectResult<ObjectRef> {
        self.fd_table
            .lock()
            .get(index as usize)
            .ok_or(ObjectError::DoesNotExist)?
            .as_ref()
            .map(|entry| entry.object.clone())
            .ok_or(ObjectError::DoesNotExist)
    }

    pub fn set_fd_entry(
        &mut self,
        slot: usize,
        object: ObjectRef,
        fd_flags: FdFlags,
    ) -> ObjectResult<usize> {
        let mut fd_table = self.fd_table.lock();
        if fd_table.len() <= slot {
            fd_table.resize(slot + 1, None);
        }
        if let Some(old_entry) = fd_table[slot].take() {
            release_fd_entry_locks(self.pid, &old_entry);
        }
        fd_table[slot] = Some(FdEntry::new(object, fd_flags));
        Ok(slot)
    }

    pub fn clear_fd_slot(&mut self, slot: usize) -> ObjectResult<()> {
        let mut fd_table = self.fd_table.lock();
        let entry = fd_table
            .get_mut(slot)
            .ok_or(ObjectError::DoesNotExist)?;
        let Some(old_entry) = entry.take() else {
            return Err(ObjectError::DoesNotExist);
        };
        release_fd_entry_locks(self.pid, &old_entry);
        Ok(())
    }

    pub fn get_fd_flags(&self, index: usize) -> ObjectResult<FdFlags> {
        self.fd_table
            .lock()
            .get(index)
            .and_then(|entry| entry.as_ref())
            .map(|entry| entry.fd_flags)
            .ok_or(ObjectError::DoesNotExist)
    }

    pub fn set_fd_flags(&mut self, index: usize, fd_flags: FdFlags) -> ObjectResult<()> {
        let mut fd_table = self.fd_table.lock();
        let entry = fd_table
            .get_mut(index)
            .and_then(|entry| entry.as_mut())
            .ok_or(ObjectError::DoesNotExist)?;
        entry.fd_flags = fd_flags;
        Ok(())
    }

    pub fn clone_object(&mut self, object: ObjectRef) -> ObjectResult<usize> {
        let slot = self.alloc_fd_slot();
        self.set_fd_entry(slot, object, FdFlags::empty())
    }

    pub fn clone_object_with_min(&mut self, object: ObjectRef, min: usize) -> ObjectResult<usize> {
        self.clone_object_with_min_and_flags(object, min, FdFlags::empty())
    }

    pub fn clone_object_with_min_and_flags(
        &mut self,
        object: ObjectRef,
        min: usize,
        fd_flags: FdFlags,
    ) -> ObjectResult<usize> {
        let slot = self.alloc_fd_slot_with_min(min);
        self.set_fd_entry(slot, object, fd_flags)
    }

    pub fn clone_object_to(&mut self, object: ObjectRef, dest: usize) -> ObjectResult<usize> {
        self.clone_object_to_with_flags(object, dest, FdFlags::empty())
    }

    pub fn clone_object_to_with_flags(
        &mut self,
        object: ObjectRef,
        dest: usize,
        fd_flags: FdFlags,
    ) -> ObjectResult<usize> {
        self.set_fd_entry(dest, object, fd_flags)
    }

    pub fn push_object(&mut self, object: ObjectRef) -> usize {
        self.push_object_with_flags(object, FdFlags::empty())
    }

    pub fn push_object_with_flags(&mut self, object: ObjectRef, fd_flags: FdFlags) -> usize {
        let slot = self.alloc_fd_slot();
        let _ = self.set_fd_entry(slot, object, fd_flags);
        slot
    }

    pub fn close_cloexec_objects(&mut self) {
        let mut fd_table = self.fd_table.lock();
        for entry in fd_table.iter_mut() {
            if entry
                .as_ref()
                .is_some_and(|entry| entry.fd_flags.contains(FdFlags::CLOEXEC))
                && let Some(old_entry) = entry.take()
            {
                release_fd_entry_locks(self.pid, &old_entry);
            }
        }
    }

    pub fn close_all_fds(&mut self) {
        let is_shared = Arc::strong_count(&self.fd_table) > 1;
        if is_shared {
            let entries = self.fd_table.lock().clone();
            release_process_fd_table_locks(self.pid, &entries);
        } else {
            let entries = {
                let mut fd_table = self.fd_table.lock();
                core::mem::take(&mut *fd_table)
            };
            for entry in entries.into_iter().flatten() {
                release_fd_entry_locks(self.pid, &entry);
            }
        }

        self.fd_table = new_fd_table();
    }
}
