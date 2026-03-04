use crate::filesystem::vfs::{FSResult, VFS};
use core::{any::Any, str::from_utf8};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use fatfs::FsOptions;
use log::trace;
use spin::Mutex;

use crate::{
    filesystem::{
        errors::FSError,
        impls::fat32::{FAT32, operator::Fat32RamDiskReader},
        path::Path,
        storage_operator::initrd::RamDiskOperator,
        vfs_traits::{
            Directory, DirectoryContentInfo, DirectoryContentType, File, FileInfo, FileLike,
            FileSystem,
        },
    },
    println, s_println,
};

impl VFS {
    pub fn create_file(&mut self, path: Path) -> FSResult<()> {
        let (parent_dir, name) = path.navigate_to_parent(self.root.clone().unwrap())?;

        parent_dir
            .clone()
            .lock()
            .create(DirectoryContentInfo::new(name, DirectoryContentType::File))
    }

    pub fn create_dir(&mut self, path: Path) -> FSResult<()> {
        let (parent_dir, name) = path.navigate_to_parent(self.root.clone().unwrap())?;

        parent_dir.clone().lock().create(DirectoryContentInfo::new(
            name,
            DirectoryContentType::Directory,
        ))
    }

    pub fn read_file(&mut self, path: Path, buffer: &mut [u8]) -> FSResult<()> {
        let file = path.navigate(self.root.clone().unwrap())?;

        if let FileLike::File(file) = file {
            file.lock().read(buffer)
        } else {
            Err(FSError::NotAFile)
        }
    }

    pub fn file_info(&mut self, path: Path) -> FSResult<FileInfo> {
        let file = path.navigate(self.root.clone().unwrap())?;

        match file {
            FileLike::File(file) => file.lock().info(),
            FileLike::Directory(_) => Err(FSError::NotAFile),
        }
    }

    pub fn write_file(&mut self, path: Path, buffer: &[u8]) -> FSResult<()> {
        let file = path.navigate(self.root.clone().unwrap())?;

        if let FileLike::File(file) = file {
            file.lock().write(buffer)
        } else {
            Err(FSError::NotAFile)
        }
    }

    pub fn delete_file(&mut self, _path: Path) -> FSResult<()> {
        unimplemented!("Just dont create files that your gonna delete lmao its not my problem")
    }

    pub fn list_contents(&self, path: Path) -> FSResult<Vec<DirectoryContentInfo>> {
        let dir = path.navigate(self.root.clone().unwrap())?;

        if let FileLike::Directory(dir) = dir {
            Ok(dir.lock().contents()?)
        } else {
            Err(FSError::NotADirectory)
        }
    }
}
