use core::fmt::Debug;

use alloc::{string::String, vec::Vec};

use crate::filesystem::vfs::{FSResult, WrappedDirectory, WrappedFile};

pub trait File: Send + Sync {
    fn info(&mut self) -> FSResult<FileLikeInfo>;

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<()>;
    fn write(&mut self, buffer: &[u8]) -> FSResult<()>;
}

pub trait Directory: Send + Sync {
    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(self.name()?, 0))
    }
    fn name(&self) -> FSResult<String>;
    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>>;
    fn create(&self, info: DirectoryContentInfo) -> FSResult<()>;
    fn delete(&self, name: &str) -> FSResult<()>;
    fn get(&self, name: &str) -> FSResult<FileLike>;
}

#[derive(Debug)]
pub struct DirectoryContentInfo {
    pub name: String,
    pub content_type: DirectoryContentType,
}

#[derive(Debug)]
pub struct FileLikeInfo {
    pub name: String,
    pub size: usize,
}

impl FileLikeInfo {
    pub fn new(name: String, size: usize) -> Self {
        Self { name, size }
    }
}

impl DirectoryContentInfo {
    pub fn new(name: String, content_type: DirectoryContentType) -> Self {
        Self { name, content_type }
    }
}

#[derive(Debug)]
pub enum DirectoryContentType {
    File,
    Directory,
    Symlink,
}

pub trait FileSystem: Send + Sync {
    fn init(&mut self) -> FSResult<()>;
    fn root_dir(&mut self) -> FSResult<WrappedDirectory>;
}

pub enum FileLike {
    File(WrappedFile),
    Directory(WrappedDirectory),
}

impl FileLike {
    pub fn info(&self) -> FSResult<FileLikeInfo> {
        match self {
            FileLike::File(file) => file.lock().info(),
            FileLike::Directory(dir) => dir.lock().info(),
        }
    }
}
