use core::fmt::Debug;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::object::misc::ObjectRef;
use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, LinuxStat},
        staticfs::{
            device::StaticDeviceHandle, directory::StaticDirectoryHandle, file::StaticFileHandle,
        },
        vfs::{FSResult, VirtualFS, WrappedDirectory, WrappedFile},
        vfs_traits::{FileLike, Whence},
    },
    impl_cast_function, impl_cast_function_non_trait,
    memory::{addrspace::mem_area::Data, protection::Protection},
    object::{
        Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, MemoryMappable, Readable, Seekable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::misc::with_current_process,
};

pub struct FileLikeObject {
    file: FileLike,
    path: crate::filesystem::path::Path,
}

fn mount_device_id_for_path(path: &crate::filesystem::path::Path) -> u64 {
    let Ok(mount_path) = VirtualFS.lock().mount_path(path.clone()) else {
        return 1;
    };

    match mount_path.as_string().as_str() {
        "/" => 1,
        "/run" => 6,
        "/proc" => 2,
        "/sys" => 3,
        "/sys/fs/cgroup" => 4,
        "/dev" => 5,
        _ => 1,
    }
}

fn stat_with_mount_device_id(
    mut stat: LinuxStat,
    path: &crate::filesystem::path::Path,
) -> LinuxStat {
    stat.st_dev = mount_device_id_for_path(path);
    stat
}

impl FileLikeObject {
    pub fn new(file: FileLike, path: crate::filesystem::path::Path) -> Self {
        Self { file, path }
    }

    pub fn path(&self) -> crate::filesystem::path::Path {
        self.path.clone()
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
            symlink.lock().read_link_target()
        } else {
            Err(FSError::NotASymlink)
        }
    }

    pub fn is_static_fs(&self) -> bool {
        match &self.file {
            FileLike::File(file) => {
                let file = file.lock();
                file.as_any().is::<StaticDeviceHandle>() || file.as_any().is::<StaticFileHandle>()
            }
            FileLike::Directory(directory) => {
                directory.lock().as_any().is::<StaticDirectoryHandle>()
            }
            FileLike::Symlink(_) => true,
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

    pub fn chmod(&self, mode: u32) -> FSResult<()> {
        match &self.file {
            FileLike::File(file) => file.lock().chmod(mode),
            FileLike::Directory(dir) => dir.lock().chmod(mode),
            FileLike::Symlink(symlink) => {
                let target = symlink.lock().target()?;
                let nested = VirtualFS.lock().open(target)?;
                nested.chmod(mode)
            }
        }
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

    fn resolve_device_object(&self) -> FSResult<Option<ObjectRef>> {
        match &self.file {
            FileLike::File(file) => {
                let file = file.lock();
                let Some(device) = file.as_any().downcast_ref::<StaticDeviceHandle>() else {
                    return Ok(None);
                };
                Ok(Some(device.object()?))
            }
            FileLike::Symlink(symlink) => {
                let target = symlink.lock().target()?;
                let nested = VirtualFS.lock().open(target)?;
                nested.resolve_device_object()
            }
            FileLike::Directory(_) => Ok(None),
        }
    }
}

pub fn poll_identity_object(object: ObjectRef) -> ObjectRef {
    if let Ok(file_like) = object.clone().as_file_like()
        && let Ok(Some(device)) = file_like.resolve_device_object()
    {
        return device;
    }

    object
}

impl Debug for FileLikeObject {
    fn fmt(&self, _f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Ok(())
    }
}

impl Object for FileLikeObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<crate::object::FileFlags> {
        let Some(device) = self.resolve_device_object()? else {
            return Err(ObjectError::Unimplemented);
        };

        device.clone().get_flags()
    }

    fn set_flags(self: Arc<Self>, flags: crate::object::FileFlags) -> ObjectResult<()> {
        let Some(device) = self.resolve_device_object()? else {
            return Err(ObjectError::Unimplemented);
        };

        device.clone().set_flags(flags)
    }

    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("mappable", MemoryMappable);
    impl_cast_function!("seekable", Seekable);
    impl_cast_function!("statable", Statable);

    impl_cast_function_non_trait!("file_like", FileLikeObject);
}

impl Writable for FileLikeObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        if let Some(device) = self.resolve_device_object()? {
            let writable = device
                .as_writable()
                .map_err(|_| ObjectError::InvalidArguments)?;
            return writable.write(buffer);
        }

        self.resolve_file()?
            .lock()
            .write(buffer)
            .map_err(Into::into)
    }
}

impl Readable for FileLikeObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        if let Some(device) = self.resolve_device_object()? {
            let readable = device
                .as_readable()
                .map_err(|_| ObjectError::InvalidArguments)?;
            return readable.read(buffer);
        }

        self.resolve_file()?.lock().read(buffer).map_err(Into::into)
    }
}

impl MemoryMappable for FileLikeObject {
    fn map(
        self: alloc::sync::Arc<Self>,
        offset: u64,
        pages: u64,
        protection: Protection,
    ) -> ObjectResult<x86_64::VirtAddr> {
        if let Some(device) = self.resolve_device_object()? {
            let mappable = device
                .as_mappable()
                .map_err(|_| ObjectError::InvalidArguments)?;
            return mappable.map(offset, pages, protection);
        }

        with_current_process(|process| {
            let file_bytes = self
                .info()
                .map(|info| (info.size as u64).saturating_sub(offset).min(pages * 4096))
                .unwrap_or(0);
            let data = Data::File {
                offset,
                file_bytes,
                file: self,
            };
            let addr = process
                .addrspace
                .allocate_user_lazy(pages, protection, data);

            Ok(addr)
        })
    }
}

impl Configuratable for FileLikeObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        let Some(device) = self.resolve_device_object()? else {
            return Err(ObjectError::InvalidRequest);
        };

        let configurable = device
            .as_configuratable()
            .map_err(|_| ObjectError::InvalidRequest)?;
        configurable.configure(request)
    }
}

impl Pollable for FileLikeObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        self.resolve_device_object()
            .ok()
            .flatten()
            .and_then(|device| device.as_pollable().ok())
            .is_some_and(|pollable| pollable.is_event_ready(event))
    }
}

impl Seekable for FileLikeObject {
    fn seek(self: alloc::sync::Arc<Self>, offset: i64, seek_type: Whence) -> ObjectResult<usize> {
        if let Ok(Some(device)) = self.resolve_device_object() {
            let seekable = device
                .as_seekable()
                .map_err(|_| ObjectError::FSError(FSError::NotAFile))?;
            return seekable.seek(offset, seek_type);
        }

        if let FileLike::File(file) = &self.file {
            file.lock().seek(offset, seek_type).map_err(Into::into)
        } else {
            Err(ObjectError::FSError(FSError::NotAFile))
        }
    }
}

impl Statable for FileLikeObject {
    fn stat(&self) -> LinuxStat {
        if let Ok(Some(device)) = self.resolve_device_object()
            && let Ok(statable) = device.as_statable()
        {
            return stat_with_mount_device_id(statable.stat(), &self.path);
        }

        stat_with_mount_device_id(
            self.info().map(FileLikeInfo::as_linux).unwrap_or_default(),
            &self.path,
        )
    }
}
