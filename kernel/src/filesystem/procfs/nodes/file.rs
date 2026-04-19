use core::any::Any;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};

pub(super) struct ProcFile {
    name: String,
    inode: u64,
    mode: u32,
    read: Arc<dyn Fn() -> Vec<u8> + Send + Sync>,
    write: Option<Arc<dyn Fn(&[u8]) -> FSResult<usize> + Send + Sync>>,
    offset: usize,
}

impl ProcFile {
    pub(super) fn new(
        name: String,
        inode: u64,
        mode: u32,
        read: Arc<dyn Fn() -> Vec<u8> + Send + Sync>,
        write: Option<Arc<dyn Fn(&[u8]) -> FSResult<usize> + Send + Sync>>,
    ) -> Self {
        Self {
            name,
            inode,
            mode,
            read,
            write,
            offset: 0,
        }
    }
}

impl File for ProcFile {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.name.clone(),
            (self.read)().len(),
            UnixPermission(self.mode),
            FileLikeType::File,
        )
        .with_inode(self.inode))
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let data = (self.read)();
        let offset = offset as usize;
        if offset >= data.len() {
            return Ok(0);
        }

        let len = buffer.len().min(data.len() - offset);
        buffer[..len].copy_from_slice(&data[offset..offset + len]);
        Ok(len)
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let read = self.read_at(buffer, self.offset as u64)?;
        self.offset += read;
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        match &self.write {
            Some(write) => write(buffer),
            None => Err(FSError::Readonly),
        }
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = (self.read)().len() as i64;
        let next = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.offset as i64 + offset,
            Whence::End => len + offset,
        }
        .max(0) as usize;

        self.offset = next;
        Ok(self.offset)
    }
}
