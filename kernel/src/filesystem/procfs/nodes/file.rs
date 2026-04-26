use core::any::Any;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};

type ProcReadCallback = dyn Fn() -> Vec<u8> + Send + Sync;
type ProcWriteCallback = dyn Fn(&[u8]) -> FSResult<usize> + Send + Sync;

pub(super) struct ProcFile {
    name: String,
    inode: u64,
    mode: u32,
    read: Arc<ProcReadCallback>,
    write: Option<Arc<ProcWriteCallback>>,
    offset: usize,
}

impl ProcFile {
    pub(super) fn new(
        name: String,
        inode: u64,
        mode: u32,
        read: Arc<ProcReadCallback>,
        write: Option<Arc<ProcWriteCallback>>,
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
            // procfs files are generated on demand; reporting a dynamic size here
            // would force content generation during metadata queries and can
            // recurse back into procfs/VFS internals such as /proc/*/mountinfo.
            0,
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
            Whence::Data => {
                if offset < 0 || offset >= len {
                    return Err(FSError::Other);
                }
                offset
            }
            Whence::Hole => {
                if offset < 0 || offset > len {
                    return Err(FSError::Other);
                }
                len
            }
        }
        .max(0) as usize;

        self.offset = next;
        Ok(self.offset)
    }
}
