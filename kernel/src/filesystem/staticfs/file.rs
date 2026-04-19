use core::any::Any;

use crate::filesystem::{
    info::{FileLikeInfo, UnixPermission},
    staticfs::StaticFileNode,
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};

pub struct StaticFileHandle {
    node: &'static StaticFileNode,
    offset: usize,
}

impl StaticFileHandle {
    pub fn new(node: &'static StaticFileNode) -> Self {
        Self { node, offset: 0 }
    }
}

impl File for StaticFileHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.node.name.into(),
            (self.node.read)().len(),
            UnixPermission(self.node.mode),
            FileLikeType::File,
        )
        .with_inode(self.node.inode))
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let data = (self.node.read)();
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

    fn write(&mut self, _buffer: &[u8]) -> FSResult<usize> {
        self.node.write.map(|write| write(_buffer)).unwrap_or(Err(crate::filesystem::errors::FSError::Readonly))
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = (self.node.read)().len() as i64;
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
