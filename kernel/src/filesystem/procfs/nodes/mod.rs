use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{info::DirectoryContentInfo, vfs_traits::FileLike};

mod directory;
mod file;
mod symlink;

use directory::ProcDirectory;
use file::ProcFile;
use symlink::ProcSymlink;

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
        Arc::new(read),
    ))))
}

pub(super) fn proc_symlink(name: &str, inode: u64, target: String) -> FileLike {
    FileLike::Symlink(Arc::new(Mutex::new(ProcSymlink::new(
        name.into(),
        inode,
        target,
    ))))
}
