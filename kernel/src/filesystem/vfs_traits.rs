use core::fmt::Debug;

use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::vfs::{FSResult, WrappedDirectory, WrappedFile};

pub trait File: Send + Sync {
    fn name(&mut self) -> FSResult<String>;
    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize>;
    fn write(&mut self, buffer: &[u8]) -> FSResult<usize>;
}

pub trait Directory: Send + Sync {
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
}

pub enum FileLike {
    File(WrappedFile),
    Directory(WrappedDirectory),
}
