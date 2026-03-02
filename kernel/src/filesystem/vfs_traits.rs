use core::fmt::Debug;

use alloc::{string::String, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::filesystem::vfs::FSResult;

pub trait File: Send + Sync + Debug {
    fn name(&self) -> FSResult<String>;
    fn read(&self, buffer: &mut [u8]) -> FSResult<usize>;
    fn write(&mut self, buffer: &[u8]) -> FSResult<usize>;
}

pub trait Directory: Send + Sync + Debug {
    fn name(&self) -> FSResult<String>;
    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>>;
    fn create(&self, info: DirectoryContentInfo) -> FSResult<()>;
    fn delete(&self, name: &str) -> FSResult<()>;
    fn get(&self, name: &str) -> FSResult<FileLike>;
}

pub struct DirectoryContentInfo {
    pub name: String,
    pub content_type: DirectoryContentType,
}

pub enum DirectoryContentType {
    File,
    Directory,
    Symlink,
}

pub trait FileSystem: Send + Sync {
    fn init(&mut self) -> FSResult<()>;
}

#[derive(Debug)]
pub enum FileLike {
    File(Arc<Mutex<dyn File>>),
    Directory(Arc<Mutex<dyn Directory>>),
}
