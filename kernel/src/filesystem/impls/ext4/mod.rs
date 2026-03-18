use alloc::{string::ToString, sync::Arc};
use spin::mutex::Mutex;

use ext4plus::Ext4;

use crate::filesystem::{
    impls::ext4::{directory::Ext4Directory, file::Ext4File, operator::Ext4RamDiskReader},
    vfs::WrappedDirectory,
    vfs_traits::FileSystem,
};

pub mod directory;
pub mod file;
pub mod operator;

/// Wrapper around the `ext4plus::Ext4` filesystem so it can be used
/// through the kernel's generic `FileSystem` trait.
pub struct EXT4(pub Ext4);

impl FileSystem for EXT4 {
    fn init(&mut self) -> crate::filesystem::vfs::FSResult<()> {
        Ok(())
    }

    fn root_dir(&mut self) -> crate::filesystem::vfs::FSResult<WrappedDirectory> {
        // The root directory path is always `/` for ext4.
        let dir = Ext4Directory::new("".to_string(), "/".to_string(), self.0.clone());
        Ok(Arc::new(Mutex::new(dir)))
    }
}

// The underlying ext4plus types are `Send + Sync`, so it is safe to
// share them behind our trait objects.
unsafe impl Sync for EXT4 {}
unsafe impl Sync for Ext4File {}
unsafe impl Send for Ext4File {}
unsafe impl Send for Ext4Directory {}
unsafe impl Sync for Ext4Directory {}

