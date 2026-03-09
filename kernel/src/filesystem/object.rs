use core::fmt::Debug;

use alloc::{boxed::Box, sync::Arc};
use spin::Mutex;

use crate::{
    filesystem::{path::Path, vfs::VirtualFS, vfs_traits::File},
    is_readable, is_writable,
    object::{Object, Readable, Writable},
    s_println,
};

pub struct FileObject {
    file: Arc<Mutex<dyn File>>,
}

impl FileObject {
    pub fn new(file: Arc<Mutex<dyn File>>) -> Self {
        Self { file }
    }
}

impl Debug for FileObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}

impl Object for FileObject {
    is_writable!();
    is_readable!();
}

impl Writable for FileObject {
    fn write(&self, buffer: &[u8]) -> crate::object::ObjectResult<usize> {
        Ok(self.file.lock().write(buffer).unwrap())
    }
}

impl Readable for FileObject {
    fn read(&self, buffer: &mut [u8]) -> crate::object::ObjectResult<usize> {
        Ok(self.file.lock().read(buffer).unwrap())
    }
}
