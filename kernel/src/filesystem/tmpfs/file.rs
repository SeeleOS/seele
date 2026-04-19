use core::any::Any;

use alloc::string::String;

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, UnixPermission},
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};

use super::{S_IFMT, TMPFS_STATE, TmpNodeKind, node_name};

pub(crate) struct TmpfsFileHandle {
    path: String,
    offset: usize,
}

impl TmpfsFileHandle {
    pub(crate) fn new(path: String) -> Self {
        Self { path, offset: 0 }
    }
}

impl File for TmpfsFileHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        let state = TMPFS_STATE.lock();
        let node = state.node(&self.path)?;
        match &node.kind {
            TmpNodeKind::File { data, mode } => Ok(FileLikeInfo::new(
                node_name(&self.path),
                data.len(),
                UnixPermission(*mode),
                FileLikeType::File,
            )
            .with_inode(node.inode)),
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => Err(FSError::NotAFile),
        }
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let state = TMPFS_STATE.lock();
        let node = state.node(&self.path)?;
        let data = match &node.kind {
            TmpNodeKind::File { data, .. } => data,
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => {
                return Err(FSError::NotAFile);
            }
        };
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
        let mut state = TMPFS_STATE.lock();
        let node = state.node_mut(&self.path)?;
        let data = match &mut node.kind {
            TmpNodeKind::File { data, .. } => data,
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => {
                return Err(FSError::NotAFile);
            }
        };
        let end = self
            .offset
            .checked_add(buffer.len())
            .ok_or(FSError::Other)?;
        if end > data.len() {
            data.resize(end, 0);
        }
        data[self.offset..end].copy_from_slice(buffer);
        self.offset = end;
        Ok(buffer.len())
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = {
            let state = TMPFS_STATE.lock();
            let node = state.node(&self.path)?;
            match &node.kind {
                TmpNodeKind::File { data, .. } => data.len() as i64,
                TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => {
                    return Err(FSError::NotAFile);
                }
            }
        };
        let next = match seek_type {
            Whence::Start => offset,
            Whence::Current => self.offset as i64 + offset,
            Whence::End => len + offset,
        }
        .max(0) as usize;
        self.offset = next;
        Ok(self.offset)
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let mut state = TMPFS_STATE.lock();
        if (mode & S_IFMT) != 0 {
            state.update_file_mode(&self.path, mode)
        } else {
            state.update_file_mode(&self.path, mode & 0o7777)
        }
    }
}
