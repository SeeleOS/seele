use core::fmt::Debug;

use alloc::{boxed::Box, sync::Arc};
use spin::Mutex;

use crate::{
    filesystem::{
        info::FileLikeInfo,
        path::Path,
        vfs::{FSResult, VirtualFS},
        vfs_traits::File,
    },
    impl_cast_function,
    object::{
        Object,
        misc::ObjectResult,
        traits::{HaveLinuxStat, Readable, Writable},
    },
    s_println,
};

pub struct FileObject {
    file: Arc<Mutex<dyn File>>,
}

impl FileObject {
    pub fn new(file: Arc<Mutex<dyn File>>) -> Self {
        Self { file }
    }

    pub fn info(&self) -> FSResult<FileLikeInfo> {
        self.file.lock().info()
    }
}

impl Debug for FileObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}

impl Object for FileObject {
    impl_cast_function!(writable, Writable);
    impl_cast_function!(readable, Readable);
    impl_cast_function!(have_linux_stat, HaveLinuxStat);
}

impl Writable for FileObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        Ok(self.file.lock().write(buffer).unwrap())
    }
}

impl Readable for FileObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        Ok(self.file.lock().read(buffer).unwrap())
    }
}

impl HaveLinuxStat for FileObject {
    fn stat(&self) -> ObjectResult<super::info::LinuxStat> {
        Ok(self.file.lock().info().unwrap().as_linux())
    }
}
