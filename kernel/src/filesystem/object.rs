use core::fmt::Debug;

use alloc::vec::Vec;

use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo},
        vfs::FSResult,
        vfs_traits::FileLike,
    },
    impl_cast_function, impl_cast_function_non_trait,
    memory::addrspace::mem_area::Data,
    object::{
        Object,
        error::ObjectError,
        misc::ObjectResult,
        traits::{MemoryMappable, Readable, Writable},
    },
    process::misc::with_current_process,
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

    pub fn directory_contents(&self) -> ObjectResult<Vec<DirectoryContentInfo>> {
        match &self.file {
            FileLike::File(_) => Err(ObjectError::FSError(FSError::NotADirectory)),
            FileLike::Directory(dir) => Ok(dir.lock().contents()?),
        }
    }

    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> FSResult<usize> {
        match &self.file {
            FileLike::File(file) => file.lock().read_at(buf, offset),
            FileLike::Directory(_) => Err(FSError::NotAFile),
        }
    }

    pub fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> FSResult<usize> {
        match &self.file {
            FileLike::File(file) => {
                let len = buf.len();
                let mut read = 0;
                let mut file = file.lock();

                while read < len {
                    let bytes_read = file.read_at(&mut buf[read..], offset + read as u64)?;
                    if bytes_read == 0 {
                        return Err(FSError::Other);
                    }
                    read += bytes_read;
                }

                Ok(read)
            }
            FileLike::Directory(_) => Err(FSError::NotAFile),
        }
    }
}

impl Debug for FileLikeObject {
    fn fmt(&self, _f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}

impl Object for FileLikeObject {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("mappable", MemoryMappable);

    impl_cast_function_non_trait!("file_like", FileLikeObject);
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

impl MemoryMappable for FileLikeObject {
    fn map(
        self: alloc::sync::Arc<Self>,
        offset: u64,
        pages: u64,
        permissions: seele_sys::permission::Permissions,
    ) -> ObjectResult<x86_64::VirtAddr> {
        with_current_process(|process| {
            let data = Data::File { offset, file: self };
            let addr = process
                .addrspace
                .allocate_user_lazy(pages, permissions, data);

            Ok(addr)
        })
    }
}
