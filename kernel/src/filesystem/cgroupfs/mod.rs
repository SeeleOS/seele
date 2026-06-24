mod content;
mod directory;
mod file;
mod file_kind;
mod state;

use core::any::Any;

use crate::memory::utils::Mut;
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use crate::{
    filesystem::{
        errors::FSError,
        info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
        path::{Path, PathPart},
        vfs::FSResult,
        vfs::VirtualFS,
        vfs_traits::{
            Directory, DirectoryContentType, File, FileLike, FileLikeType, FileSystem, Whence,
        },
    },
    process::{manager::MANAGER, misc::ProcessID},
};

use content::{absolute_cgroup_path, file_contents, file_info, write_file};
use directory::CgroupDirectoryHandle;
use file::CgroupFileHandle;
use file_kind::CgroupFileKind;
use state::{CGROUP_STATE, CgroupState};

const ROOT_INODE: u64 = 0x6000_0000;
const DEFAULT_DIR_MODE: u32 = 0o040755;
const READONLY_FILE_MODE: u32 = 0o100444;
const WRITABLE_FILE_MODE: u32 = 0o100644;

pub struct CgroupFs;

impl CgroupFs {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CgroupFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for CgroupFs {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let absolute = absolute_cgroup_path(path);
        if absolute == "/" {
            return Ok(FileLike::Directory(Arc::new(Mut::new(
                CgroupDirectoryHandle::new("/".into()),
            ))));
        }

        let parent = Path::new(&absolute)
            .parent()
            .unwrap_or_default()
            .as_string();
        let name = Path::new(&absolute)
            .file_name()
            .ok_or(FSError::NotFound)?
            .to_string();

        if CGROUP_STATE.lock().directories.contains_key(&absolute) {
            return Ok(FileLike::Directory(Arc::new(Mut::new(
                CgroupDirectoryHandle::new(absolute),
            ))));
        }

        let kind = CgroupFileKind::from_name(&name).ok_or(FSError::NotFound)?;
        CGROUP_STATE.lock().directory(&parent)?;
        Ok(FileLike::File(Arc::new(Mut::new(CgroupFileHandle::new(
            parent, kind,
        )))))
    }

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn link(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn name(&self) -> &'static str {
        "cgroup2"
    }

    fn magic(&self) -> i64 {
        0x6367_7270
    }

    fn mount_source(&self) -> &'static str {
        "cgroup2"
    }

    fn default_mount_flags(&self, _path: &Path) -> crate::filesystem::vfs_traits::MountFlags {
        crate::filesystem::vfs_traits::MountFlags::MS_NOSUID
            | crate::filesystem::vfs_traits::MountFlags::MS_NODEV
            | crate::filesystem::vfs_traits::MountFlags::MS_NOEXEC
            | crate::filesystem::vfs_traits::MountFlags::MS_RELATIME
    }
}

pub fn pid_cgroup_path(pid: ProcessID) -> String {
    CGROUP_STATE.lock().pid_path(pid)
}

pub fn set_pid_cgroup_path_from_fs_path(pid: ProcessID, path: &Path) -> FSResult<()> {
    let normalized = path.normalize();
    let (mount_path, fs, _, _) = VirtualFS.lock().mount_metadata(normalized.clone())?;
    if fs.lock().as_any().downcast_ref::<CgroupFs>().is_none() {
        return Err(FSError::NotFound);
    };

    let cgroup_path = normalized
        .strip_prefix(&mount_path)
        .unwrap_or_default()
        .normalize()
        .as_string();

    CGROUP_STATE.lock().set_pid_path(pid, &cgroup_path)
}

pub fn remove_pid_cgroup_path(pid: ProcessID) {
    CGROUP_STATE.lock().remove_pid_path(pid);
}
