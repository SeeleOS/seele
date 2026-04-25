use alloc::vec::Vec;

use crate::filesystem::{vfs::FSResult, vfs_traits::DirectoryContentType};

type StaticWriteFn = fn(&[u8]) -> FSResult<usize>;

pub struct StaticDirEntry {
    pub name: &'static str,
    pub node: &'static StaticNode,
}

pub struct StaticDirectoryNode {
    pub name: &'static str,
    pub inode: u64,
    pub mode: u32,
    pub entries: &'static [StaticDirEntry],
}

pub struct StaticFileNode {
    pub name: &'static str,
    pub inode: u64,
    pub mode: u32,
    pub read: fn() -> Vec<u8>,
    pub write: Option<StaticWriteFn>,
}

pub struct StaticSymlinkNode {
    pub name: &'static str,
    pub inode: u64,
    pub mode: u32,
    pub target: &'static str,
}

pub struct StaticDeviceNode {
    pub name: &'static str,
    pub inode: u64,
    pub mode: u32,
    pub device_name: &'static str,
    pub rdev: Option<u64>,
}

pub enum StaticNode {
    Directory(StaticDirectoryNode),
    File(StaticFileNode),
    Symlink(StaticSymlinkNode),
    Device(StaticDeviceNode),
}

impl StaticNode {
    pub fn content_type(&self) -> DirectoryContentType {
        match self {
            Self::Directory(_) => DirectoryContentType::Directory,
            Self::File(_) | Self::Device(_) => DirectoryContentType::File,
            Self::Symlink(_) => DirectoryContentType::Symlink,
        }
    }
}
