use alloc::{string::String, sync::Arc, vec::Vec};
use spin::mutex::Mutex;

use crate::filesystem::{
    errors::FSError,
    impls::fat32::{file::FAT32File, operator::Fat32RamDiskReader},
    info::DirectoryContentInfo,
    vfs_traits::{Directory, DirectoryContentType, FileLike},
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

    fn contents(&self) -> crate::filesystem::vfs::FSResult<alloc::vec::Vec<DirectoryContentInfo>> {
        log::trace!("fat32 dir contents");
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
        log::trace!("fat32 dir create {}", info.name);
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
        log::trace!("fat32 dir delete {}", name);
        self.inner.remove(name).map_err(|_| FSError::NotFound)
    }

    fn get(
        &self,
        name: &str,
    ) -> crate::filesystem::vfs::FSResult<crate::filesystem::vfs_traits::FileLike> {
        log::trace!("fat32 dir get {}", name);
        let name = name.to_ascii_uppercase();

        for dir_entry in self.inner.iter() {
            let dir_entry = dir_entry.unwrap();

            if dir_entry.file_name() == name {
                if dir_entry.is_file() {
                    return Ok(FileLike::File(Arc::new(Mutex::new(FAT32File::new(
                        dir_entry.file_name(),
                        dir_entry.to_file(),
                        dir_entry.len() as usize,
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
