use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{
    errors::FSError,
    impls::ext4::{EXT4, operator::Ext4RamDiskOperator},
    storage_operator::initrd::RamDiskOperator,
    vfs_traits::{Directory, File, FileSystem},
};
use ext4plus::Ext4 as Ext4Inner;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref VirtualFS: Mutex<VFS> = Mutex::new(VFS::new());
}
// INode: pointer to file and file info
// Superblock: basicaly metadata for the partition
// 目录在文件系统中是一种特殊的文件，它的内容是一个列表，列表中的每一项都是一个“目录项”（directory entry），每个目录项记录一个文件名和对应的Inode编号。
//
// Getting a file:
// file path: /home/seele/file.txt
// Get INode (root) -> FileContent (Directory) -> directoy contents -> INode(to ./seele)
// INode (to .seele) -> Seele -> contents -> INode(file.txt) -> Contents
pub type FSResult<T> = Result<T, FSError>;
pub type WrappedDirectory = Arc<Mutex<dyn Directory>>;
pub type WrappedFile = Arc<Mutex<dyn File>>;

pub struct VFS {
    pub root: Option<WrappedDirectory>,
    pub filesystems: Vec<Box<Mutex<dyn FileSystem>>>,
}

impl VFS {
    pub fn new() -> Self {
        Self {
            root: None,
            filesystems: Vec::new(),
        }
    }

    pub fn init(&mut self) -> FSResult<()> {
        log::debug!("vfs: init start");
        // 使用 ext4plus + RamDiskOperator 作为根文件系统。
        let reader = Ext4RamDiskOperator(Mutex::new(RamDiskOperator::default()));
        let writer = Ext4RamDiskOperator(Mutex::new(RamDiskOperator::default()));
        let ext4 = Ext4Inner::load_with_writer(Box::new(reader), Some(Box::new(writer))).unwrap();
        self.register_fs(EXT4(ext4));

        self.root = Some(self.filesystems[0].lock().root_dir().unwrap());

        log::debug!("vfs: init done");
        Ok(())
    }

    fn register_fs(&mut self, fs: impl FileSystem + 'static) {
        log::debug!("vfs: register filesystem");
        self.filesystems.push(Box::new(Mutex::new(fs)));
    }
}
