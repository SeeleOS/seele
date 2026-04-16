use core::fmt::Debug;

use alloc::string::String;
use alloc::vec::Vec;

use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, LinuxStat},
        vfs::{FSResult, VirtualFS, WrappedDirectory, WrappedFile},
        vfs_traits::FileLike,
    },
    impl_cast_function, impl_cast_function_non_trait,
    memory::addrspace::mem_area::Data,
    object::{
        Object,
        error::ObjectError,
        misc::ObjectResult,
        traits::{MemoryMappable, Readable, Seekable, Statable, Writable},
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
            FileLike::Symlink(symlink) => symlink.lock().info(),
        }
    }

    pub fn directory_contents(&self) -> ObjectResult<Vec<DirectoryContentInfo>> {
        self.resolve_dir()?.lock().contents().map_err(Into::into)
    }

    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> FSResult<usize> {
        self.resolve_file()?.lock().read_at(buf, offset)
    }

    pub fn read_link(&self) -> FSResult<String> {
        if let FileLike::Symlink(symlink) = &self.file {
            Ok(symlink.lock().target()?.as_string())
        } else {
            Err(FSError::NotASymlink)
        }
    }

    pub fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> FSResult<usize> {
        let file = self.resolve_file()?;
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

    fn resolve_file(&self) -> FSResult<WrappedFile> {
        match &self.file {
            FileLike::File(file) => Ok(file.clone()),
            FileLike::Symlink(symlink) => {
                let target = symlink.lock().target()?;
                VirtualFS.lock().resolve_file(target)
            }
            FileLike::Directory(_) => Err(FSError::NotAFile),
        }
    }

    fn resolve_dir(&self) -> FSResult<WrappedDirectory> {
        match &self.file {
            FileLike::Directory(dir) => Ok(dir.clone()),
            FileLike::Symlink(symlink) => {
                let target = symlink.lock().target()?;
                VirtualFS.lock().resolve_dir(target)
            }
            FileLike::File(_) => Err(FSError::NotADirectory),
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
    impl_cast_function!("seekable", Seekable);
    impl_cast_function!("statable", Statable);

    impl_cast_function_non_trait!("file_like", FileLikeObject);
}

impl Writable for FileLikeObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        self.resolve_file()?
            .lock()
            .write(buffer)
            .map_err(Into::into)
    }
}

impl Readable for FileLikeObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.resolve_file()?.lock().read(buffer).map_err(Into::into)
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
            let file_bytes = self
                .info()
                .map(|info| (info.size as u64).saturating_sub(offset))
                .unwrap_or(0);
            let data = Data::File {
                offset,
                file_bytes,
                file: self,
            };
            let addr = process
                .addrspace
                .allocate_user_lazy(pages, permissions, data);

            Ok(addr)
        })
    }
}

impl Seekable for FileLikeObject {
    fn seek(
        self: alloc::sync::Arc<Self>,
        offset: i64,
        seek_type: seele_sys::abi::object::SeekType,
    ) -> ObjectResult<usize> {
        if let FileLike::File(file) = &self.file {
            file.lock().seek(offset, seek_type).map_err(Into::into)
        } else {
            Err(ObjectError::FSError(FSError::NotAFile))
        }
    }
}

impl Statable for FileLikeObject {
    fn stat(&self) -> LinuxStat {
        self.info().map(FileLikeInfo::as_linux).unwrap_or_default()
    }
}
