use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::{
    block_device::BlockDevice,
    block_device::cache::CachedBlockDevice,
    errors::FSError,
    impls::ext4::{EXT4, operator::Ext4BlockOperator},
    vfs_traits::{Directory, File, FileSystem, Symlink},
};
use ext4plus::Ext4 as Ext4Inner;
use lazy_static::lazy_static;

use crate::drivers::virtio::block::root_device as virtio_root_device;

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
pub type WrappedSymlink = Arc<Mutex<dyn Symlink>>;

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
        let block_device: Arc<dyn BlockDevice> = Arc::new(CachedBlockDevice::new(
            virtio_root_device().ok_or(FSError::NotFound)?,
        ));
        log::info!("vfs: loading ext4 from root block device");
        let reader = Ext4BlockOperator::new(block_device.clone());
        let writer = Ext4BlockOperator::new(block_device);
        let ext4 = Ext4Inner::load_with_writer(Box::new(reader), Some(Box::new(writer))).unwrap();
        log::info!("vfs: ext4 loaded");
        self.register_fs(EXT4(ext4));

        log::info!("vfs: building root dir");
        self.root = Some(self.filesystems[0].lock().root_dir().unwrap());
        log::info!("vfs: root dir ready");

        if let Err(err) = self.clear_directory(crate::filesystem::path::Path::new("/tmp")) {
            log::warn!("vfs: failed to clean /tmp: {:?}", err);
        } else {
            log::info!("vfs: cleaned /tmp");
        }

        log::debug!("vfs: init done");
        Ok(())
    }

    fn register_fs(&mut self, fs: impl FileSystem + 'static) {
        log::debug!("vfs: register filesystem");
        self.filesystems.push(Box::new(Mutex::new(fs)));
    }
}
