use core::any::Any;
use core::fmt::Debug;

use alloc::{boxed::Box, string::String, vec::Vec};
use num_enum::TryFromPrimitive;

use crate::filesystem::{
    errors::FSError,
    info::{DirectoryContentInfo, FileLikeInfo, UnixPermission},
    path::Path,
    vfs::{FSResult, WrappedDirectory, WrappedFile, WrappedSymlink},
};

#[repr(u64)]
#[derive(Clone, Copy, TryFromPrimitive, Debug)]
pub enum Whence {
    Start = 0,
    Current = 1,
    End = 2,
}

pub trait File: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn info(&mut self) -> FSResult<FileLikeInfo>;

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize>;
    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize>;
    fn write(&mut self, buffer: &[u8]) -> FSResult<usize>;
    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize>;
    fn chmod(&self, _mode: u32) -> FSResult<()> {
        Err(FSError::Readonly)
    }
}

pub trait Directory: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn info(&self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.name()?,
            0,
            UnixPermission::directory(),
            FileLikeType::Directory,
        ))
    }
    fn name(&self) -> FSResult<String>;
    fn contents(&self) -> FSResult<Vec<DirectoryContentInfo>>;
    fn create(&self, info: DirectoryContentInfo) -> FSResult<()>;
    fn create_symlink(&self, _name: &str, _target: &str) -> FSResult<()> {
        Err(FSError::Readonly)
    }
    fn delete(&self, name: &str) -> FSResult<()>;
    fn get(&self, name: &str) -> FSResult<FileLike>;
    fn chmod(&self, _mode: u32) -> FSResult<()> {
        Err(FSError::Readonly)
    }
}

pub trait Symlink: Send + Sync {
    fn info(&self) -> FSResult<FileLikeInfo>;
    fn target(&self) -> FSResult<Path>;
    fn read_link_target(&self) -> FSResult<String> {
        Ok(self.target()?.as_string())
    }
    fn chmod(&self, _mode: u32) -> FSResult<()> {
        Err(FSError::Readonly)
    }
}

#[derive(Clone, Debug)]
pub enum DirectoryContentType {
    File,
    Directory,
    Symlink,
}

pub trait FileSystem: Send + Sync {
    fn init(&mut self) -> FSResult<()>;
    fn lookup(&self, path: &Path) -> FSResult<FileLike>;
    fn name(&self) -> &'static str;
    fn magic(&self) -> i64;
    fn mount_source(&self) -> &'static str;
    fn mount_options(&self, path: &Path) -> &'static str;
}

#[derive(Debug)]
pub enum FileLikeType {
    File,
    Directory,
    Symlink,
}

pub enum FileLike {
    File(WrappedFile),
    Directory(WrappedDirectory),
    Symlink(WrappedSymlink),
}

impl FileLike {
    pub fn info(&self) -> FSResult<FileLikeInfo> {
        match self {
            FileLike::File(file) => file.lock().info(),
            FileLike::Directory(dir) => dir.lock().info(),
            FileLike::Symlink(symlink) => symlink.lock().info(),
        }
    }
}
