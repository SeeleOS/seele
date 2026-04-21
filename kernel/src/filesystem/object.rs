use core::fmt::{Debug, Formatter, Result as FmtResult};

use alloc::{string::String, sync::Arc, vec::Vec};
use x86_64::VirtAddr;

use crate::object::misc::ObjectRef;
use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, LinuxStat},
        path::Path,
        staticfs::{
            device::StaticDeviceHandle, directory::StaticDirectoryHandle, file::StaticFileHandle,
        },
        vfs::{FSResult, VirtualFS, WrappedDirectory, WrappedFile},
        vfs_traits::{FileLike, Whence},
    },
    impl_cast_function, impl_cast_function_non_trait,
    memory::{addrspace::mem_area::Data, protection::Protection},
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, MemoryMappable, Readable, Seekable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::misc::with_current_process,
};

pub struct OpenedFileObject {
    backend: OpenBackend,
    path: Path,
}

pub type FileLikeObject = OpenedFileObject;

enum OpenBackend {
    RegularFile(WrappedFile),
    Device {
        file: WrappedFile,
        object: ObjectRef,
    },
    Directory(WrappedDirectory),
    SymlinkPath {
        target: Path,
        info: FileLikeInfo,
    },
}

fn device_object_for_file(file: &WrappedFile) -> FSResult<Option<ObjectRef>> {
    let file = file.lock();
    let Some(device) = file.as_any().downcast_ref::<StaticDeviceHandle>() else {
        return Ok(None);
    };
    Ok(Some(device.object()?))
}

pub(crate) fn mount_device_id_for_path(path: &Path) -> u64 {
    let Ok(device_id) = VirtualFS.lock().mount_device_id(path.clone()) else {
        return 1;
    };
    device_id
}

fn stat_with_mount_device_id(mut stat: LinuxStat, path: &Path) -> LinuxStat {
    stat.st_dev = mount_device_id_for_path(path);
    stat
}

impl OpenBackend {
    fn from_file_like(file: FileLike) -> FSResult<Self> {
        match file {
            FileLike::File(file) => {
                if let Some(object) = device_object_for_file(&file)? {
                    Ok(Self::Device { file, object })
                } else {
                    Ok(Self::RegularFile(file))
                }
            }
            FileLike::Directory(dir) => Ok(Self::Directory(dir)),
            FileLike::Symlink(symlink) => {
                let symlink = symlink.lock();
                Ok(Self::SymlinkPath {
                    target: symlink.target()?,
                    info: symlink.info()?,
                })
            }
        }
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        match self {
            Self::RegularFile(file) | Self::Device { file, .. } => file.lock().info(),
            Self::Directory(dir) => dir.lock().info(),
            Self::SymlinkPath { info, .. } => Ok(info.clone()),
        }
    }
}

impl OpenedFileObject {
    pub fn new(file: FileLike, path: Path) -> FSResult<Self> {
        Ok(Self {
            backend: OpenBackend::from_file_like(file)?,
            path,
        })
    }

    pub fn path(&self) -> Path {
        self.path.clone()
    }

    pub fn info(&self) -> FSResult<FileLikeInfo> {
        self.backend.info()
    }

    pub fn directory_contents(&self) -> ObjectResult<Vec<DirectoryContentInfo>> {
        self.resolve_dir()?.lock().contents().map_err(Into::into)
    }

    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> FSResult<usize> {
        self.resolve_file()?.lock().read_at(buf, offset)
    }

    pub fn read_link(&self) -> FSResult<String> {
        match &self.backend {
            OpenBackend::SymlinkPath { target, .. } => Ok(target.clone().as_string()),
            _ => Err(FSError::NotASymlink),
        }
    }

    pub fn is_static_fs(&self) -> bool {
        match &self.backend {
            OpenBackend::RegularFile(file) => {
                let file = file.lock();
                file.as_any().is::<StaticFileHandle>()
            }
            OpenBackend::Device { .. } => true,
            OpenBackend::Directory(directory) => {
                directory.lock().as_any().is::<StaticDirectoryHandle>()
            }
            OpenBackend::SymlinkPath { .. } => true,
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
        if self.device_object().is_some() {
            let _ = mode;
            return Ok(());
        }

        match &self.backend {
            OpenBackend::RegularFile(file) => file.lock().chmod(mode),
            OpenBackend::Device { .. } => Ok(()),
            OpenBackend::Directory(dir) => dir.lock().chmod(mode),
            OpenBackend::SymlinkPath { target, .. } => {
                let nested = VirtualFS.lock().open(target.clone())?;
                nested.chmod(mode)
            }
        }
    }

    pub fn truncate(&self, length: u64) -> FSResult<()> {
        self.resolve_file()?.lock().truncate(length)
    }

    pub fn allocate(&self, mode: u32, offset: u64, len: u64) -> FSResult<()> {
        self.resolve_file()?.lock().allocate(mode, offset, len)
    }

    fn resolve_file(&self) -> FSResult<WrappedFile> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => Ok(file.clone()),
            OpenBackend::SymlinkPath { target, .. } => {
                VirtualFS.lock().resolve_file(target.clone())
            }
            OpenBackend::Directory(_) => Err(FSError::NotAFile),
        }
    }

    fn resolve_dir(&self) -> FSResult<WrappedDirectory> {
        match &self.backend {
            OpenBackend::Directory(dir) => Ok(dir.clone()),
            OpenBackend::SymlinkPath { target, .. } => VirtualFS.lock().resolve_dir(target.clone()),
            OpenBackend::RegularFile(_) | OpenBackend::Device { .. } => Err(FSError::NotADirectory),
        }
    }

    fn device_object(&self) -> Option<ObjectRef> {
        match &self.backend {
            OpenBackend::Device { object, .. } => Some(object.clone()),
            OpenBackend::RegularFile(_)
            | OpenBackend::Directory(_)
            | OpenBackend::SymlinkPath { .. } => None,
        }
    }
}

pub fn poll_identity_object(object: ObjectRef) -> ObjectRef {
    if let Ok(file_like) = object.clone().as_file_like()
        && let Some(device) = file_like.device_object()
    {
        return device;
    }

    object
}

impl Debug for OpenedFileObject {
    fn fmt(&self, _f: &mut Formatter<'_>) -> FmtResult {
        Ok(())
    }
}

impl Object for OpenedFileObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        let Some(device) = self.device_object() else {
            return Err(ObjectError::Unimplemented);
        };

        device.clone().get_flags()
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        let Some(device) = self.device_object() else {
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

impl Writable for OpenedFileObject {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        if let Some(device) = self.device_object() {
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

impl Readable for OpenedFileObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        if let Some(device) = self.device_object() {
            let readable = device
                .as_readable()
                .map_err(|_| ObjectError::InvalidArguments)?;
            return readable.read(buffer);
        }

        self.resolve_file()?.lock().read(buffer).map_err(Into::into)
    }
}

impl MemoryMappable for OpenedFileObject {
    fn map(
        self: Arc<Self>,
        offset: u64,
        pages: u64,
        protection: Protection,
    ) -> ObjectResult<VirtAddr> {
        if let Some(device) = self.device_object() {
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

impl Configuratable for OpenedFileObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        let Some(device) = self.device_object() else {
            return Err(ObjectError::InvalidRequest);
        };

        let configurable = device
            .as_configuratable()
            .map_err(|_| ObjectError::InvalidRequest)?;
        configurable.configure(request)
    }
}

impl Pollable for OpenedFileObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        self.device_object()
            .and_then(|device| device.as_pollable().ok())
            .is_some_and(|pollable| pollable.is_event_ready(event))
    }
}

impl Seekable for OpenedFileObject {
    fn seek(self: Arc<Self>, offset: i64, seek_type: Whence) -> ObjectResult<usize> {
        if let Some(device) = self.device_object() {
            let seekable = device
                .as_seekable()
                .map_err(|_| ObjectError::FSError(FSError::NotAFile))?;
            return seekable.seek(offset, seek_type);
        }

        match &self.backend {
            OpenBackend::RegularFile(file) => {
                file.lock().seek(offset, seek_type).map_err(Into::into)
            }
            OpenBackend::Device { .. }
            | OpenBackend::Directory(_)
            | OpenBackend::SymlinkPath { .. } => Err(ObjectError::FSError(FSError::NotAFile)),
        }
    }
}

impl Statable for OpenedFileObject {
    fn stat(&self) -> LinuxStat {
        if let Some(device) = self.device_object() {
            let mut stat = self.info().map(FileLikeInfo::as_linux).unwrap_or_default();
            if let Ok(statable) = device.as_statable() {
                stat.st_rdev = statable.stat().st_rdev;
            }
            return stat_with_mount_device_id(stat, &self.path);
        }

        stat_with_mount_device_id(
            self.info().map(FileLikeInfo::as_linux).unwrap_or_default(),
            &self.path,
        )
    }
}
