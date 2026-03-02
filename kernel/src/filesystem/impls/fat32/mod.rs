use alloc::{boxed::Box, string::ToString, sync::Arc};
use fatfs::{DefaultTimeProvider, LossyOemCpConverter};
use spin::mutex::Mutex;

use crate::filesystem::{
    impls::fat32::{directory::FAT32Directory, file::FAT32File, operator::Fat32RamDiskReader},
    vfs::WrappedDirectory,
    vfs_traits::FileSystem,
};

pub mod directory;
pub mod file;
pub mod operator;

pub struct FAT32(pub fatfs::FileSystem<Fat32RamDiskReader>);
impl FileSystem for FAT32 {
    fn init(&mut self) -> crate::filesystem::vfs::FSResult<()> {
        Ok(())
    }

    fn root_dir(&mut self) -> crate::filesystem::vfs::FSResult<WrappedDirectory> {
        let root = self.0.root_dir();
        let static_root = unsafe {
            core::mem::transmute::<
                fatfs::Dir<'_, Fat32RamDiskReader, DefaultTimeProvider, LossyOemCpConverter>,
                fatfs::Dir<'static, Fat32RamDiskReader, DefaultTimeProvider, LossyOemCpConverter>,
            >(root)
        };
        Ok(Arc::new(Mutex::new(FAT32Directory::new(
            "".to_string(),
            static_root,
        ))))
    }
}

unsafe impl Sync for FAT32 {}
unsafe impl Sync for FAT32File {}
unsafe impl Send for FAT32File {}
unsafe impl Send for FAT32Directory {}
unsafe impl Sync for FAT32Directory {}
