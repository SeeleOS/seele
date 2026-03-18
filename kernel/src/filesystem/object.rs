use core::fmt::Debug;

use alloc::{boxed::Box, sync::Arc};
use spin::Mutex;

use crate::{
    filesystem::{
        errors::FSError,
        info::FileLikeInfo,
        path::Path,
        vfs::{FSResult, VirtualFS},
        vfs_traits::{File, FileLike},
    },
    impl_cast_function, impl_cast_function_non_trait,
    object::{
        Object,
        error::ObjectError,
        misc::ObjectResult,
        traits::{HaveLinuxStat, Readable, Writable},
    },
    s_println,
};

pub struct FileLikeObject {
    file: FileLike,
}

impl FileLikeObject {
    pub fn new(file: FileLike) -> Self {
        Self { file }
    }

    pub fn info(&self) -> FSResult<FileLikeInfo> {
        match &self.file {
            FileLike::File(file) => file.lock().info(),
            FileLike::Directory(dir) => dir.lock().info(),
        }
    }
}

impl Debug for FileLikeObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}

impl Object for FileLikeObject {
    impl_cast_function!(writable, Writable);
    impl_cast_function!(readable, Readable);
    impl_cast_function!(have_linux_stat, HaveLinuxStat);

    impl_cast_function_non_trait!(file_like, FileLikeObject);
}

impl Writable for FileLikeObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        match &self.file {
            FileLike::File(file) => Ok(file.lock().write(buffer)?),
            FileLike::Directory(_) => Err(ObjectError::FSError(FSError::NotAFile)),
        }
    }
}

impl Readable for FileLikeObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        match &self.file {
            FileLike::File(file) => Ok(file.lock().read(buffer)?),
            FileLike::Directory(_) => Err(ObjectError::FSError(FSError::NotAFile)),
        }
    }
}

impl HaveLinuxStat for FileLikeObject {
    fn stat(&self) -> ObjectResult<super::info::LinuxStat> {
        match &self.file {
            FileLike::File(f) => Ok(f.lock().info()?.as_linux()),
            FileLike::Directory(d) => Ok(d.lock().info()?.as_linux()),
        }
    }
}
