use alloc::{string::String, sync::Arc, vec::Vec};
use fatfs::{IoBase, Read, Seek, Write};
use spin::mutex::Mutex;

use crate::filesystem::{
    block_device::BlockDeviceError,
    errors::FSError,
    impls::fat32::{file::FAT32File, operator::Fat32RamDiskReader},
    storage_operator::{SeekFrom, StorageOperator, initrd::RamDiskOperator},
    vfs_traits::{
        Directory, DirectoryContentInfo, DirectoryContentType, File, FileLike, FileSystem,
    },
};

type RawFAT32Directory<'a> =
    fatfs::Dir<'a, Fat32RamDiskReader, fatfs::DefaultTimeProvider, fatfs::LossyOemCpConverter>;

pub struct FAT32Directory {
    name: String,
    inner: RawFAT32Directory<'static>,
}

impl FAT32Directory {
    pub fn new(name: String, inner: RawFAT32Directory<'static>) -> Self {
        Self { name, inner }
    }
}

impl Directory for FAT32Directory {
    fn name(&self) -> crate::filesystem::vfs::FSResult<String> {
        Ok(self.name.clone())
    }

    fn contents(
        &self,
    ) -> crate::filesystem::vfs::FSResult<
        alloc::vec::Vec<crate::filesystem::vfs_traits::DirectoryContentInfo>,
    > {
        let mut contents = Vec::new();

        for dir_entry in self.inner.iter() {
            contents.push(DirectoryContentInfo {
                name: dir_entry.as_ref().unwrap().file_name().clone(),
                content_type: if dir_entry.unwrap().is_file() {
                    DirectoryContentType::File
                } else {
                    DirectoryContentType::Directory
                },
            });
        }

        Ok(contents)
    }

    fn create(&self, info: DirectoryContentInfo) -> crate::filesystem::vfs::FSResult<()> {
        match info.content_type {
            DirectoryContentType::File => {
                self.inner.create_file(&info.name).unwrap();
            }
            DirectoryContentType::Directory => {
                self.inner.create_dir(&info.name).unwrap();
            }
            _ => unimplemented!(),
        }

        Ok(())
    }

    fn delete(&self, name: &str) -> crate::filesystem::vfs::FSResult<()> {
        self.inner.remove(name).map_err(|_| FSError::NotFound)
    }

    fn get(
        &self,
        name: &str,
    ) -> crate::filesystem::vfs::FSResult<crate::filesystem::vfs_traits::FileLike> {
        for dir_entry in self.inner.iter() {
            let dir_entry = dir_entry.unwrap();

            if dir_entry.file_name() == name {
                if dir_entry.is_file() {
                    return Ok(FileLike::File(Arc::new(Mutex::new(FAT32File::new(
                        dir_entry.file_name(),
                        dir_entry.to_file(),
                    )))));
                } else if dir_entry.is_dir() {
                    return Ok(FileLike::Directory(Arc::new(Mutex::new(
                        FAT32Directory::new(dir_entry.file_name(), dir_entry.to_dir()),
                    ))));
                }
            }
        }

        Err(FSError::NotFound)
    }
}
