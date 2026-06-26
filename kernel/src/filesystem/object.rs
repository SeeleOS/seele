use core::fmt::{Debug, Formatter, Result as FmtResult};

use alloc::{string::String, sync::Arc, vec::Vec};
use x86_64::VirtAddr;

use crate::object::misc::ObjectRef;
use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, FileTimes, LinuxStat},
        page_cache::{self, FileCacheIdentity, FileCacheKey},
        path::Path,
        staticfs::{
            device::StaticDeviceHandle, directory::StaticDirectoryHandle, file::StaticFileHandle,
        },
        tmpfs::TmpfsDeviceHandle,
        vfs::{FSResult, VirtualFS, WrappedDirectory, WrappedFile},
        vfs_operations::{open_path, resolve_dir_path, resolve_file_path},
        vfs_traits::{
            File as VfsFile, FileLike, FileLikeType, LinuxFileAttributes, MountFlags, Symlink,
            Whence,
        },
    },
    impl_cast_function, impl_cast_function_non_trait,
    memory::{addrspace::mem_area::Data, protection::Protection, user_safe, utils::Mut},
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        open_state::OpenState,
        traits::{Configuratable, MemoryMappable, Readable, Seekable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::{manager::get_current_process, misc::with_current_process},
};

pub struct OpenedFileObject {
    backend: OpenBackend,
    open_state: OpenState,
    directory_offset: Mut<usize>,
    path: Path,
    mount_device_id: u64,
    mount_id: u64,
    mount_root: bool,
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
        symlink: Arc<Mut<dyn Symlink>>,
        read_link_target: String,
        target: Path,
        info: FileLikeInfo,
    },
}

fn device_object_for_file(file: &WrappedFile) -> FSResult<Option<ObjectRef>> {
    let file = file.lock();
    if let Some(device) = file.as_any().downcast_ref::<StaticDeviceHandle>() {
        return Ok(Some(device.object()?));
    }
    if let Some(device) = file.as_any().downcast_ref::<TmpfsDeviceHandle>() {
        return Ok(device.object().ok());
    }
    Ok(None)
}

fn device_rdev_for_file(file: &WrappedFile) -> Option<u64> {
    let file = file.lock();
    if let Some(device) = file.as_any().downcast_ref::<StaticDeviceHandle>() {
        return device.rdev();
    }
    file.as_any()
        .downcast_ref::<TmpfsDeviceHandle>()
        .map(TmpfsDeviceHandle::rdev)
}

pub(crate) fn mount_device_id_for_path(path: &Path) -> u64 {
    let Ok((_, device_id, _)) = VirtualFS.lock().mount_path_and_ids(path.clone()) else {
        return 1;
    };
    device_id
}

fn stat_with_mount_device_id(mut stat: LinuxStat, mount_device_id: u64) -> LinuxStat {
    stat.st_dev = mount_device_id;
    stat
}

impl OpenBackend {
    fn from_wrapped_file(file: WrappedFile) -> FSResult<Self> {
        if let Some(object) = device_object_for_file(&file)? {
            Ok(Self::Device { file, object })
        } else {
            Ok(Self::RegularFile(file))
        }
    }

    fn symlink_target_from_path(path: &Path, target: &str) -> Path {
        let target_path = Path::new(target);
        if target_path.is_absolute() {
            return target_path;
        }

        let mut combined = path.parent().unwrap_or_default().as_string();
        if !combined.ends_with('/') {
            combined.push('/');
        }
        combined.push_str(target);
        Path::new(&combined).normalize()
    }

    fn from_file_like(file: FileLike, path: &Path) -> FSResult<Self> {
        match file {
            FileLike::File(file) => Self::from_wrapped_file(file),
            FileLike::Directory(dir) => Ok(Self::Directory(dir)),
            FileLike::Symlink(symlink) => {
                let symlink_handle = symlink.clone();
                let symlink = symlink_handle.lock();
                let read_link_target = symlink.read_link_target()?;
                Ok(Self::SymlinkPath {
                    symlink: symlink_handle.clone(),
                    target: Self::symlink_target_from_path(path, &read_link_target),
                    read_link_target,
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

fn reject_xattr_on_protected_file(attributes: LinuxFileAttributes) -> FSResult<()> {
    if attributes
        .intersects(LinuxFileAttributes::FS_IMMUTABLE_FL | LinuxFileAttributes::FS_APPEND_FL)
    {
        return Err(FSError::PermissionDenied);
    }
    Ok(())
}

impl OpenedFileObject {
    fn rlimit_fsize_cur() -> u64 {
        get_current_process().lock().rlimit_fsize_cur
    }

    fn limit_write_len_for_rlimit(offset: u64, requested_len: usize) -> ObjectResult<usize> {
        let limit = Self::rlimit_fsize_cur();
        if limit <= offset {
            return Err(ObjectError::Other);
        }

        Ok(requested_len.min((limit - offset) as usize))
    }

    fn is_device_mode(mode: u32) -> bool {
        const S_IFMT: u32 = 0o170000;
        const S_IFCHR: u32 = 0o020000;
        const S_IFBLK: u32 = 0o060000;
        let file_type = mode & S_IFMT;
        matches!(file_type, S_IFCHR | S_IFBLK)
            || mode != 0 && file_type == 0 && (mode & (S_IFCHR | S_IFBLK) != 0)
    }

    fn is_fifo_mode(mode: u32) -> bool {
        const S_IFMT: u32 = 0o170000;
        const S_IFIFO: u32 = 0o010000;
        mode & S_IFMT == S_IFIFO || mode != 0 && mode & S_IFMT == 0 && mode & S_IFIFO == S_IFIFO
    }

    fn from_backend(
        path: Path,
        backend: OpenBackend,
        mount_device_id: u64,
        mount_id: u64,
        mount_root: bool,
    ) -> Self {
        Self {
            backend,
            open_state: OpenState::default(),
            directory_offset: Mut::new(0),
            path,
            mount_device_id,
            mount_id,
            mount_root,
        }
    }

    pub(crate) fn write_all_to_cursor(
        file: &mut dyn VfsFile,
        buf: &[u8],
        offset: u64,
    ) -> FSResult<usize> {
        file.seek(offset as i64, Whence::Start)?;

        let mut written = 0usize;
        while written < buf.len() {
            let count = file.write(&buf[written..])?;
            if count == 0 {
                return Err(FSError::NoSpace);
            }
            written += count;
        }

        Ok(written)
    }

    pub(crate) fn new_with_mount_device_id(
        file: FileLike,
        path: Path,
        mount_device_id: u64,
        mount_id: u64,
        mount_root: bool,
        mount_flags: MountFlags,
    ) -> FSResult<Self> {
        let info = file.info()?;
        if mount_flags.contains(MountFlags::MS_NODEV)
            && (Self::is_device_mode(info.permission.0) || info.rdev != 0)
        {
            return Err(FSError::AccessDenied);
        }
        Ok(Self::from_backend(
            path.clone(),
            OpenBackend::from_file_like(file, &path)?,
            mount_device_id,
            mount_id,
            mount_root,
        ))
    }

    pub fn new(file: FileLike, path: Path) -> FSResult<Self> {
        let mount_device_id = mount_device_id_for_path(&path);
        Ok(Self::from_backend(
            path.clone(),
            OpenBackend::from_file_like(file, &path)?,
            mount_device_id,
            1,
            false,
        ))
    }

    pub fn path(&self) -> Path {
        self.path.clone()
    }

    pub fn mount_id(&self) -> u64 {
        self.mount_id
    }

    pub fn mount_root(&self) -> bool {
        self.mount_root
    }

    pub fn info(&self) -> FSResult<FileLikeInfo> {
        self.backend.info()
    }

    pub fn directory_contents(&self) -> ObjectResult<Vec<DirectoryContentInfo>> {
        self.resolve_dir()?.lock().contents().map_err(Into::into)
    }

    pub fn directory_offset(&self, entry_count: usize) -> usize {
        let mut offset = self.directory_offset.lock();
        let current_offset = (*offset).min(entry_count);
        *offset = current_offset;
        current_offset
    }

    pub fn advance_directory_offset(&self, count: usize) {
        let mut offset = self.directory_offset.lock();
        *offset = offset.saturating_add(count);
    }

    pub fn read_at(&self, buf: &mut [u8], offset: u64) -> FSResult<usize> {
        if let Some((file, identity)) = self.readonly_page_cache_file() {
            return page_cache::read(&file, identity, buf, offset);
        }

        self.resolve_file()?.lock().read_at(buf, offset)
    }

    pub fn read_link(&self) -> FSResult<String> {
        match &self.backend {
            OpenBackend::SymlinkPath {
                read_link_target, ..
            } => Ok(read_link_target.clone()),
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
        let len = buf.len();
        let mut read = 0;

        while read < len {
            let bytes_read = self.read_at(&mut buf[read..], offset + read as u64)?;
            if bytes_read == 0 {
                return Err(FSError::Other);
            }
            read += bytes_read;
        }

        Ok(read)
    }

    pub fn write_at(&self, buf: &[u8], offset: u64) -> FSResult<usize> {
        if let Some(device) = self.device_object()
            && let Ok(block_device) = device.clone().as_block_device()
        {
            self.invalidate_page_cache();
            return block_device
                .write_at(buf, offset as usize)
                .map_err(|_| FSError::Other);
        }

        let len =
            Self::limit_write_len_for_rlimit(offset, buf.len()).map_err(|_| FSError::Other)?;
        self.with_file_write_cursor(true, |file| {
            Self::write_all_to_cursor(file, &buf[..len], offset)
        })
    }

    pub fn write_exact_at(&self, buf: &[u8], offset: u64) -> FSResult<usize> {
        self.write_at(buf, offset)
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
                let nested = open_path(target.clone())?;
                nested.chmod(mode)
            }
        }
    }

    pub fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        self.chown_following_symlink(uid, gid, true)
    }

    pub fn lchown(&self, uid: u32, gid: u32) -> FSResult<()> {
        self.chown_following_symlink(uid, gid, false)
    }

    fn chown_following_symlink(&self, uid: u32, gid: u32, follow_symlink: bool) -> FSResult<()> {
        if self.device_object().is_some() {
            let _ = (uid, gid);
            return Ok(());
        }

        match &self.backend {
            OpenBackend::RegularFile(file) => file.lock().chown(uid, gid),
            OpenBackend::Device { .. } => Ok(()),
            OpenBackend::Directory(dir) => dir.lock().chown(uid, gid),
            OpenBackend::SymlinkPath {
                symlink, target, ..
            } => {
                if !follow_symlink {
                    return symlink.lock().chown(uid, gid);
                }
                let nested = open_path(target.clone())?;
                nested.chown(uid, gid)
            }
        }
    }

    pub fn set_times(&self, times: FileTimes, follow_symlink: bool) -> FSResult<()> {
        if self.device_object().is_some() {
            let _ = times;
            return Ok(());
        }

        match &self.backend {
            OpenBackend::RegularFile(file) => file.lock().set_times(times),
            OpenBackend::Device { .. } => Ok(()),
            OpenBackend::Directory(dir) => dir.lock().set_times(times),
            OpenBackend::SymlinkPath {
                symlink, target, ..
            } => {
                if !follow_symlink {
                    return symlink.lock().set_times(times);
                }
                open_path(target.clone())?.set_times(times, true)
            }
        }
    }

    pub fn linux_file_attributes(&self) -> FSResult<LinuxFileAttributes> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().linux_file_attributes()
            }
            OpenBackend::Directory(_) | OpenBackend::SymlinkPath { .. } => {
                Err(FSError::InvalidArguments)
            }
        }
    }

    pub fn set_linux_file_attributes(&self, attributes: LinuxFileAttributes) -> FSResult<()> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().set_linux_file_attributes(attributes)
            }
            OpenBackend::Directory(_) | OpenBackend::SymlinkPath { .. } => {
                Err(FSError::InvalidArguments)
            }
        }
    }

    pub fn get_xattr(&self, name: &str) -> FSResult<Option<Vec<u8>>> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().get_xattr(name)
            }
            OpenBackend::Directory(dir) => dir.lock().get_xattr(name),
            OpenBackend::SymlinkPath { target, .. } => open_path(target.clone())?.get_xattr(name),
        }
    }

    pub fn lget_xattr(&self, name: &str) -> FSResult<Option<Vec<u8>>> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().get_xattr(name)
            }
            OpenBackend::Directory(dir) => dir.lock().get_xattr(name),
            OpenBackend::SymlinkPath { symlink, .. } => symlink.lock().get_xattr(name),
        }
    }

    pub fn set_xattr(
        &self,
        name: String,
        value: Vec<u8>,
        create: bool,
        replace: bool,
    ) -> FSResult<()> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                let file = file.lock();
                reject_xattr_on_protected_file(file.linux_file_attributes()?)?;
                file.set_xattr(name, value, create, replace)
            }
            OpenBackend::Directory(dir) => dir.lock().set_xattr(name, value, create, replace),
            OpenBackend::SymlinkPath { target, .. } => {
                open_path(target.clone())?.set_xattr(name, value, create, replace)
            }
        }
    }

    pub fn lset_xattr(
        &self,
        name: String,
        value: Vec<u8>,
        create: bool,
        replace: bool,
    ) -> FSResult<()> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                let file = file.lock();
                reject_xattr_on_protected_file(file.linux_file_attributes()?)?;
                file.set_xattr(name, value, create, replace)
            }
            OpenBackend::Directory(dir) => dir.lock().set_xattr(name, value, create, replace),
            OpenBackend::SymlinkPath { symlink, .. } => {
                symlink.lock().set_xattr(name, value, create, replace)
            }
        }
    }

    pub fn list_xattrs(&self) -> FSResult<Vec<String>> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().list_xattrs()
            }
            OpenBackend::Directory(dir) => dir.lock().list_xattrs(),
            OpenBackend::SymlinkPath { target, .. } => open_path(target.clone())?.list_xattrs(),
        }
    }

    pub fn llist_xattrs(&self) -> FSResult<Vec<String>> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().list_xattrs()
            }
            OpenBackend::Directory(dir) => dir.lock().list_xattrs(),
            OpenBackend::SymlinkPath { symlink, .. } => symlink.lock().list_xattrs(),
        }
    }

    pub fn remove_xattr(&self, name: &str) -> FSResult<()> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().remove_xattr(name)
            }
            OpenBackend::Directory(dir) => dir.lock().remove_xattr(name),
            OpenBackend::SymlinkPath { target, .. } => {
                open_path(target.clone())?.remove_xattr(name)
            }
        }
    }

    pub fn lremove_xattr(&self, name: &str) -> FSResult<()> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().remove_xattr(name)
            }
            OpenBackend::Directory(dir) => dir.lock().remove_xattr(name),
            OpenBackend::SymlinkPath { symlink, .. } => symlink.lock().remove_xattr(name),
        }
    }

    pub fn truncate(&self, length: u64) -> FSResult<()> {
        self.invalidate_page_cache();
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => {
                file.lock().truncate(length)
            }
            OpenBackend::Directory(_) | OpenBackend::SymlinkPath { .. } => Err(FSError::NotAFile),
        }
    }

    pub fn allocate(
        &self,
        mode: crate::filesystem::vfs_traits::FallocateMode,
        offset: u64,
        len: u64,
    ) -> FSResult<()> {
        self.invalidate_page_cache();
        self.resolve_file()?.lock().allocate(mode, offset, len)
    }

    pub fn link_to(&self, new_path: Path) -> FSResult<()> {
        let old_mount_path = VirtualFS.lock().mount_path(self.path.clone())?;
        let new_mount_path = VirtualFS.lock().mount_path(new_path.clone())?;
        if old_mount_path != new_mount_path {
            return Err(FSError::Other);
        }

        let new_relative = new_path
            .strip_prefix(&old_mount_path)
            .ok_or(FSError::NotFound)?;
        self.resolve_file()?.lock().link_to(&new_relative)
    }

    fn resolve_file(&self) -> FSResult<WrappedFile> {
        match &self.backend {
            OpenBackend::RegularFile(file) | OpenBackend::Device { file, .. } => Ok(file.clone()),
            OpenBackend::SymlinkPath { target, .. } => resolve_file_path(target.clone()),
            OpenBackend::Directory(_) => Err(FSError::NotAFile),
        }
    }

    fn resolve_dir(&self) -> FSResult<WrappedDirectory> {
        match &self.backend {
            OpenBackend::Directory(dir) => Ok(dir.clone()),
            OpenBackend::SymlinkPath { target, .. } => resolve_dir_path(target.clone()),
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

    pub fn device_backing_object(&self) -> Option<ObjectRef> {
        self.device_object()
    }

    pub fn is_device_backed(&self) -> bool {
        self.device_object().is_some()
    }

    pub(crate) fn with_file_write_cursor<R>(
        &self,
        invalidate_cache: bool,
        f: impl FnOnce(&mut dyn VfsFile) -> FSResult<R>,
    ) -> FSResult<R> {
        if invalidate_cache {
            self.invalidate_page_cache();
        }

        let file = self.resolve_file()?;
        let mut file = file.lock();
        let current = file.seek(0, Whence::Current)? as i64;
        let result = f(&mut *file);
        let _ = file.seek(current, Whence::Start);
        result
    }

    fn page_cache_identity(&self) -> Option<FileCacheIdentity> {
        let OpenBackend::RegularFile(file) = &self.backend else {
            return None;
        };

        let mut file = file.lock();
        if file.as_any().is::<StaticFileHandle>() {
            return None;
        }

        let info = file.info().ok()?;
        if !matches!(info.file_like_type, FileLikeType::File) {
            return None;
        }

        Some(FileCacheIdentity::new(
            self.mount_device_id,
            info.inode,
            info.size,
        ))
    }

    pub(crate) fn mapping_identity(&self) -> Option<FileCacheKey> {
        Some(self.page_cache_identity()?.file)
    }

    pub(crate) fn readonly_page_cache_file(&self) -> Option<(WrappedFile, FileCacheIdentity)> {
        let OpenBackend::RegularFile(file) = &self.backend else {
            return None;
        };
        Some((file.clone(), self.page_cache_identity()?))
    }

    fn invalidate_page_cache(&self) {
        if let Some(identity) = self.page_cache_identity() {
            page_cache::invalidate_file(identity.file);
        }
    }

    pub fn mmap_data(self: Arc<Self>, offset: u64, pages: u64, shared: bool) -> Data {
        let file_bytes = self
            .info()
            .map(|info| (info.size as u64).saturating_sub(offset).min(pages * 4096))
            .unwrap_or(0);
        Data::File {
            offset,
            file_bytes,
            zero_fill_after_file: false,
            file: self,
            shared,
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
        Ok(self.open_state.get_flags())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        self.open_state.set_flags(flags);
        Ok(())
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
            if let Ok(block_device) = device.clone().as_block_device() {
                self.invalidate_page_cache();
                return block_device.write_to_cursor(buffer);
            }
            let writable = device
                .as_writable()
                .map_err(|_| ObjectError::InvalidArguments)?;
            return writable.write(buffer);
        }

        self.invalidate_page_cache();
        let file = self.resolve_file()?;
        let mut file = file.lock();

        if self.open_state.contains(FileFlags::APPEND) {
            file.seek(0, Whence::End)?;
        }

        let offset = file.seek(0, Whence::Current)? as u64;
        let len = Self::limit_write_len_for_rlimit(offset, buffer.len())?;
        file.write(&buffer[..len]).map_err(Into::into)
    }
}

impl Readable for OpenedFileObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        if let Some(device) = self.device_object() {
            if let Ok(block_device) = device.clone().as_block_device() {
                return block_device.read_from_cursor(buffer);
            }
            let readable = device
                .as_readable()
                .map_err(|_| ObjectError::InvalidArguments)?;
            return readable.read_with_flags(buffer, self.open_state.get_flags());
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
            let data = self.mmap_data(offset, pages, false);
            let addr = process
                .addrspace
                .allocate_user_lazy(pages, protection, data);

            Ok(addr)
        })
    }
}

impl Configuratable for OpenedFileObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::FileGetFlags(ptr) => {
                let flags = match &self.backend {
                    OpenBackend::RegularFile(_) | OpenBackend::Device { .. } => {
                        self.linux_file_attributes()?
                    }
                    OpenBackend::Directory(_) | OpenBackend::SymlinkPath { .. } => {
                        return Err(ObjectError::InvalidRequest);
                    }
                };
                user_safe::write(ptr, &flags.bits()).map_err(|_| ObjectError::BadAddress)?;
                return Ok(0);
            }
            ConfigurateRequest::FileSetFlags(ptr) => {
                let raw = user_safe::read(ptr).map_err(|_| ObjectError::BadAddress)?;
                let flags = LinuxFileAttributes::from_bits_retain(raw);
                match &self.backend {
                    OpenBackend::RegularFile(_) | OpenBackend::Device { .. } => {
                        self.set_linux_file_attributes(flags)?
                    }
                    OpenBackend::Directory(_) | OpenBackend::SymlinkPath { .. } => {
                        return Err(ObjectError::InvalidRequest);
                    }
                }
                return Ok(0);
            }
            _ => {}
        }

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
        if let Some(device) = self.device_object() {
            return device
                .as_pollable()
                .is_ok_and(|pollable| pollable.is_event_ready(event));
        }

        matches!(
            event,
            PollableEvent::CanBeRead | PollableEvent::CanBeWritten
        )
    }

    fn supports_epoll(&self) -> bool {
        if let Some(device) = self.device_object() {
            return device
                .as_pollable()
                .is_ok_and(|pollable| pollable.supports_epoll());
        }

        let OpenBackend::RegularFile(file) = &self.backend else {
            return false;
        };
        file.lock()
            .as_any()
            .downcast_ref::<crate::filesystem::procfs::ProcFile>()
            .is_some_and(crate::filesystem::procfs::ProcFile::supports_epoll)
    }
}

impl Seekable for OpenedFileObject {
    fn seek(self: Arc<Self>, offset: i64, seek_type: Whence) -> ObjectResult<usize> {
        if let Some(device) = self.device_object() {
            let seekable = device
                .as_seekable()
                .map_err(|_| ObjectError::FSError(FSError::IllegalSeek))?;
            return seekable.seek(offset, seek_type);
        }

        match &self.backend {
            OpenBackend::RegularFile(file) => {
                if Self::is_fifo_mode(file.lock().info()?.permission.0) {
                    return Err(ObjectError::FSError(FSError::IllegalSeek));
                }
                file.lock().seek(offset, seek_type).map_err(Into::into)
            }
            OpenBackend::Device { .. }
            | OpenBackend::Directory(_)
            | OpenBackend::SymlinkPath { .. } => Err(ObjectError::FSError(FSError::IllegalSeek)),
        }
    }
}

impl Statable for OpenedFileObject {
    fn stat(&self) -> LinuxStat {
        if let Some(device) = self.device_object() {
            let mut stat = self.info().map(FileLikeInfo::as_linux).unwrap_or_default();
            if let Some(rdev) = match &self.backend {
                OpenBackend::Device { file, .. } => device_rdev_for_file(file),
                _ => None,
            } {
                stat.st_rdev = rdev;
            } else if let Ok(statable) = device.as_statable() {
                stat.st_rdev = statable.stat().st_rdev;
            }
            return stat_with_mount_device_id(stat, self.mount_device_id);
        }

        stat_with_mount_device_id(
            self.info().map(FileLikeInfo::as_linux).unwrap_or_default(),
            self.mount_device_id,
        )
    }
}
