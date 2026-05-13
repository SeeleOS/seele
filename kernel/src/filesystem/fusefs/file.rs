use core::any::Any;

use crate::memory::utils::Mut;
use alloc::{string::String, sync::Arc};

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};

use super::connection::FuseConnection;

pub struct FuseFile {
    connection: Arc<FuseConnection>,
    nodeid: u64,
    offset: Mut<u64>,
}

impl FuseFile {
    pub fn new(connection: Arc<FuseConnection>, nodeid: u64) -> Self {
        Self {
            connection,
            nodeid,
            offset: Mut::new(0),
        }
    }
}

impl File for FuseFile {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        let attr = self.connection.getattr(self.nodeid)?;
        Ok(FileLikeInfo::new(
            String::new(),
            attr.size as usize,
            UnixPermission(attr.mode),
            FileLikeType::File,
        )
        .with_owner(attr.uid, attr.gid)
        .with_inode(attr.ino))
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let data = self
            .connection
            .read_file(self.nodeid, offset, buffer.len() as u32)?;
        let len = data.len().min(buffer.len());
        buffer[..len].copy_from_slice(&data[..len]);
        Ok(len)
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let mut offset = self.offset.lock();
        let data = self
            .connection
            .read_file(self.nodeid, *offset, buffer.len() as u32)?;
        let read = data.len().min(buffer.len());
        buffer[..read].copy_from_slice(&data[..read]);
        *offset += read as u64;
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let mut offset = self.offset.lock();
        let written = self.connection.write_file(self.nodeid, *offset, buffer)?;
        *offset += written as u64;
        Ok(written)
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let mut current = self.offset.lock();
        let size = self.connection.getattr(self.nodeid)?.size as i64;
        let next = match seek_type {
            Whence::Start => offset,
            Whence::Current => *current as i64 + offset,
            Whence::End => size + offset,
            Whence::Data | Whence::Hole => return Err(FSError::IllegalSeek),
        };
        if next < 0 {
            return Err(FSError::IllegalSeek);
        }
        *current = next as u64;
        Ok(*current as usize)
    }

    fn truncate(&mut self, length: u64) -> FSResult<()> {
        let _ = self.connection.setattr_size(self.nodeid, length)?;
        Ok(())
    }

    fn allocate(&mut self, _mode: u32, _offset: u64, _len: u64) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn link_to(&self, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let _ = self
            .connection
            .setattr_mode(self.nodeid, Some(mode), None, None)?;
        Ok(())
    }
}
