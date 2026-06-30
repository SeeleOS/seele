use crate::memory::utils::Mut;
use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    filesystem::{
        info::DirectoryContentInfo, staticfs::device::StaticDeviceHandle, vfs::FSResult,
        vfs_traits::FileLike,
    },
    object::misc::ObjectRef,
};

mod directory;
mod file;
mod symlink;

use directory::ProcDirectory;
pub(crate) use file::ProcFile;
use symlink::ProcSymlink;

const PROC_FILE_MODE_READONLY: u32 = 0o100444;
const PROC_FILE_MODE_READWRITE: u32 = 0o100644;

pub(super) fn proc_dir(
    path: &str,
    name: &str,
    inode: u64,
    entries: Vec<DirectoryContentInfo>,
) -> FileLike {
    FileLike::Directory(Arc::new(Mut::new(ProcDirectory::new(
        name.into(),
        path.into(),
        inode,
        entries,
    ))))
}

pub(super) fn proc_dynamic_dir<F>(path: &str, name: &str, inode: u64, entries: F) -> FileLike
where
    F: Fn() -> Vec<DirectoryContentInfo> + Send + Sync + 'static,
{
    FileLike::Directory(Arc::new(Mut::new(ProcDirectory::new_dynamic(
        name.into(),
        path.into(),
        inode,
        Arc::new(entries),
    ))))
}

pub(super) fn proc_file<F>(name: &str, inode: u64, read: F) -> FileLike
where
    F: Fn() -> Vec<u8> + Send + Sync + 'static,
{
    proc_file_with_epoll(name, inode, read, false)
}

pub(super) fn proc_file_with_epoll<F>(
    name: &str,
    inode: u64,
    read: F,
    epoll_ready: bool,
) -> FileLike
where
    F: Fn() -> Vec<u8> + Send + Sync + 'static,
{
    FileLike::File(Arc::new(Mut::new(ProcFile::new(
        name.into(),
        inode,
        PROC_FILE_MODE_READONLY,
        Arc::new(read),
        None,
        None,
        epoll_ready,
    ))))
}

pub(super) fn proc_sparse_file<F>(name: &str, inode: u64, read_at: F) -> FileLike
where
    F: Fn(&mut [u8], u64) -> FSResult<usize> + Send + Sync + 'static,
{
    FileLike::File(Arc::new(Mut::new(ProcFile::new(
        name.into(),
        inode,
        PROC_FILE_MODE_READONLY,
        Arc::new(Vec::new),
        Some(Arc::new(read_at)),
        None,
        false,
    ))))
}

pub(super) fn proc_rw_file<F, W>(name: &str, inode: u64, read: F, write: W) -> FileLike
where
    F: Fn() -> Vec<u8> + Send + Sync + 'static,
    W: Fn(&[u8]) -> FSResult<usize> + Send + Sync + 'static,
{
    proc_rw_file_with_epoll(name, inode, read, write, false)
}

pub(super) fn proc_rw_file_with_epoll<F, W>(
    name: &str,
    inode: u64,
    read: F,
    write: W,
    epoll_ready: bool,
) -> FileLike
where
    F: Fn() -> Vec<u8> + Send + Sync + 'static,
    W: Fn(&[u8]) -> FSResult<usize> + Send + Sync + 'static,
{
    FileLike::File(Arc::new(Mut::new(ProcFile::new(
        name.into(),
        inode,
        PROC_FILE_MODE_READWRITE,
        Arc::new(read),
        None,
        Some(Arc::new(write)),
        epoll_ready,
    ))))
}

pub(super) fn proc_object_file(name: &str, inode: u64, object: ObjectRef) -> FileLike {
    FileLike::File(Arc::new(Mut::new(StaticDeviceHandle::from_object(
        name.into(),
        inode,
        PROC_FILE_MODE_READONLY,
        None,
        object,
    ))))
}

pub(super) fn proc_symlink(name: &str, inode: u64, target: String) -> FileLike {
    FileLike::Symlink(Arc::new(Mut::new(ProcSymlink::new(
        name.into(),
        inode,
        target,
    ))))
}

pub(super) fn proc_dynamic_symlink<F>(name: &str, inode: u64, target: F) -> FileLike
where
    F: Fn() -> FSResult<String> + Send + Sync + 'static,
{
    FileLike::Symlink(Arc::new(Mut::new(ProcSymlink::new_dynamic(
        name.into(),
        inode,
        Arc::new(target),
    ))))
}

pub(super) fn proc_magic_symlink<F>(name: &str, inode: u64, target: F) -> FileLike
where
    F: Fn() -> FSResult<String> + Send + Sync + 'static,
{
    FileLike::Symlink(Arc::new(Mut::new(ProcSymlink::new_magic_dynamic(
        name.into(),
        inode,
        Arc::new(target),
    ))))
}
