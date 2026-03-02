use crate::filesystem::{
    impls::fat32::{directory::FAT32Directory, file::FAT32File, operator::Fat32RamDiskReader},
    vfs_traits::FileSystem,
};

pub mod directory;
pub mod file;
pub mod operator;

pub struct FAT32(fatfs::FileSystem<Fat32RamDiskReader>);
impl FileSystem for FAT32 {
    fn init(&mut self) -> crate::filesystem::vfs::FSResult<()> {
        Ok(())
    }
}

unsafe impl Sync for FAT32 {}
unsafe impl Sync for FAT32File {}
unsafe impl Send for FAT32File {}
unsafe impl Send for FAT32Directory {}
unsafe impl Sync for FAT32Directory {}
