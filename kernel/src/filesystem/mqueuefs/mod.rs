use core::any::Any;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
        path::{Path, PathPart},
        vfs::FSResult,
        vfs_traits::{
            Directory, DirectoryContentType, File, FileLike, FileLikeType, FileSystem, MountFlags,
            Whence,
        },
    },
    memory::utils::Mut,
    process::manager::get_current_process,
    systemcall::implementations::posix_mq::{
        PosixMessageQueueObject, create_posix_mqueue, get_posix_mqueue, list_posix_mqueues,
        unlink_posix_mqueue,
    },
};

const MQUEUE_SUPER_MAGIC: i64 = 0x1980_0202;
const ROOT_INODE: u64 = 0x4d51_0000;

pub struct MqueueFs {
    ipc_namespace_inode: u64,
}

impl MqueueFs {
    pub fn new_for_current_process() -> Self {
        Self {
            ipc_namespace_inode: get_current_process().lock().ipc_namespace.inode(),
        }
    }

    fn root(&self) -> FileLike {
        FileLike::Directory(Arc::new(Mut::new(MqueueDirectory::new(
            self.ipc_namespace_inode,
        ))))
    }
}

impl FileSystem for MqueueFs {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let components = relative_components(path);
        match components.as_slice() {
            [] => Ok(self.root()),
            [name] => MqueueDirectory::new(self.ipc_namespace_inode).get(name),
            _ => Err(FSError::NotADirectory),
        }
    }

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::OperationNotSupported)
    }

    fn link(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::OperationNotSupported)
    }

    fn name(&self) -> &'static str {
        "mqueue"
    }

    fn magic(&self) -> i64 {
        MQUEUE_SUPER_MAGIC
    }

    fn mount_source(&self) -> &'static str {
        "mqueue"
    }

    fn default_mount_flags(&self, _path: &Path) -> MountFlags {
        MountFlags::MS_NOSUID
            | MountFlags::MS_NODEV
            | MountFlags::MS_NOEXEC
            | MountFlags::MS_RELATIME
    }
}

struct MqueueDirectory {
    ipc_namespace_inode: u64,
}

impl MqueueDirectory {
    fn new(ipc_namespace_inode: u64) -> Self {
        Self {
            ipc_namespace_inode,
        }
    }
}

impl Directory for MqueueDirectory {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            String::new(),
            0,
            UnixPermission(0o755),
            FileLikeType::Directory,
        )
        .with_inode(ROOT_INODE))
    }

    fn name(&self) -> FSResult<String> {
        Ok(String::new())
    }

    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>> {
        Ok(list_posix_mqueues(self.ipc_namespace_inode)
            .into_iter()
            .map(|(name, queue)| {
                DirectoryContentInfo::new(name, DirectoryContentType::File)
                    .with_inode(queue.inode())
            })
            .collect())
    }

    fn create(&self, info: DirectoryContentInfo) -> FSResult<()> {
        require_current_ipc_namespace(self.ipc_namespace_inode)?;
        match info.content_type {
            DirectoryContentType::File => create_posix_mqueue(
                self.ipc_namespace_inode,
                &info.name,
                info.permission.unwrap_or(UnixPermission(0o600)).0,
            )
            .map(|_| ())
            .map_err(fs_error_from_syscall),
            DirectoryContentType::Directory | DirectoryContentType::Symlink => {
                Err(FSError::Readonly)
            }
        }
    }

    fn delete(&self, name: &str) -> FSResult<()> {
        require_current_ipc_namespace(self.ipc_namespace_inode)?;
        unlink_posix_mqueue(self.ipc_namespace_inode, name).map_err(fs_error_from_syscall)
    }

    fn get(&self, name: &str) -> FSResult<FileLike> {
        let queue =
            get_posix_mqueue(self.ipc_namespace_inode, name).map_err(fs_error_from_syscall)?;
        Ok(FileLike::File(Arc::new(Mut::new(MqueueFile::new(
            String::from(name),
            queue,
        )))))
    }
}

struct MqueueFile {
    name: String,
    queue: Arc<PosixMessageQueueObject>,
}

impl MqueueFile {
    fn new(name: String, queue: Arc<PosixMessageQueueObject>) -> Self {
        Self { name, queue }
    }
}

impl File for MqueueFile {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        Ok(self.queue.file_info(self.name.clone()))
    }

    fn read_at(&mut self, _buffer: &mut [u8], _offset: u64) -> FSResult<usize> {
        Err(FSError::OperationNotSupported)
    }

    fn read(&mut self, _buffer: &mut [u8]) -> FSResult<usize> {
        Err(FSError::OperationNotSupported)
    }

    fn write(&mut self, _buffer: &[u8]) -> FSResult<usize> {
        Err(FSError::OperationNotSupported)
    }

    fn seek(&mut self, _offset: i64, _seek_type: Whence) -> FSResult<usize> {
        Err(FSError::IllegalSeek)
    }

    fn truncate(&mut self, length: u64) -> FSResult<()> {
        if length == 0 {
            Ok(())
        } else {
            Err(FSError::OperationNotSupported)
        }
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        self.queue.chmod(mode);
        Ok(())
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        self.queue.chown(uid, gid);
        Ok(())
    }
}

fn relative_components(path: &Path) -> Vec<String> {
    path.normalize()
        .parts
        .iter()
        .filter_map(|part| match part {
            PathPart::Normal(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

fn require_current_ipc_namespace(ipc_namespace_inode: u64) -> FSResult<()> {
    let current = get_current_process().lock().ipc_namespace.inode();
    if current == ipc_namespace_inode {
        Ok(())
    } else {
        Err(FSError::AccessDenied)
    }
}

fn fs_error_from_syscall(error: crate::systemcall::utils::SyscallError) -> FSError {
    match error {
        crate::systemcall::utils::SyscallError::FileNotFound => FSError::NotFound,
        crate::systemcall::utils::SyscallError::FileAlreadyExists => FSError::AlreadyExists,
        crate::systemcall::utils::SyscallError::PermissionDenied => FSError::PermissionDenied,
        crate::systemcall::utils::SyscallError::InvalidArguments => FSError::InvalidArguments,
        _ => FSError::Other,
    }
}
