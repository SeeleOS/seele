use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{info::DirectoryContentInfo, vfs::FSResult, vfs_traits::FileLike};

mod directory;
mod file;
mod symlink;

use directory::ProcDirectory;
use file::ProcFile;
use symlink::ProcSymlink;

const PROC_FILE_MODE_READONLY: u32 = 0o100444;
const PROC_FILE_MODE_READWRITE: u32 = 0o100644;

pub(super) fn proc_dir(name: &str, inode: u64, entries: Vec<DirectoryContentInfo>) -> FileLike {
    FileLike::Directory(Arc::new(Mutex::new(ProcDirectory::new(
        name.into(),
        inode,
        entries,
    ))))
}

pub(super) fn proc_file<F>(name: &str, inode: u64, read: F) -> FileLike
where
    F: Fn() -> Vec<u8> + Send + Sync + 'static,
{
    FileLike::File(Arc::new(Mutex::new(ProcFile::new(
        name.into(),
        inode,
        PROC_FILE_MODE_READONLY,
        Arc::new(read),
        None,
    ))))
}

pub(super) fn proc_rw_file<F, W>(name: &str, inode: u64, read: F, write: W) -> FileLike
where
    F: Fn() -> Vec<u8> + Send + Sync + 'static,
    W: Fn(&[u8]) -> FSResult<usize> + Send + Sync + 'static,
{
    FileLike::File(Arc::new(Mutex::new(ProcFile::new(
        name.into(),
        inode,
        PROC_FILE_MODE_READWRITE,
        Arc::new(read),
        Some(Arc::new(write)),
    ))))
}

pub(super) fn proc_symlink(name: &str, inode: u64, target: String) -> FileLike {
    FileLike::Symlink(Arc::new(Mutex::new(ProcSymlink::new(
        name.into(),
        inode,
        target,
    ))))
}
