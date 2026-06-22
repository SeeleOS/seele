use core::any::Any;

use alloc::{string::String, vec::Vec};

use crate::filesystem::{
    errors::FSError,
    info::{FileLikeInfo, FileTimes, UnixPermission},
    path::Path,
    vfs::FSResult,
    vfs_traits::{File, FileLikeType, Whence},
};

use super::{S_IFMT, TmpNodeKind, TmpfsStateRef, node_name};

pub(crate) struct TmpfsFileHandle {
    state: TmpfsStateRef,
    path: String,
    inode: u64,
    offset: usize,
}

impl TmpfsFileHandle {
    pub(crate) fn new(state: TmpfsStateRef, path: String, inode: u64) -> Self {
        Self {
            state,
            path,
            inode,
            offset: 0,
        }
    }
}

impl Drop for TmpfsFileHandle {
    fn drop(&mut self) {
        let _ = self.state.lock().release_inode(self.inode);
    }
}

impl File for TmpfsFileHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        let state = self.state.lock();
        let node = state.node_by_inode(self.inode)?;
        match &node.kind {
            TmpNodeKind::File { data, mode, rdev } => Ok(FileLikeInfo::new(
                node_name(&self.path),
                data.len(),
                UnixPermission(*mode),
                FileLikeType::File,
            )
            .with_inode(node.inode)
            .with_owner(node.uid, node.gid)
            .with_rdev(*rdev)
            .with_times(node.times)),
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => Err(FSError::NotAFile),
        }
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let state = self.state.lock();
        let node = state.node_by_inode(self.inode)?;
        let data = match &node.kind {
            TmpNodeKind::File { data, .. } => data,
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => {
                return Err(FSError::NotAFile);
            }
        };
        Ok(data.read_at(buffer, offset as usize))
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let read = self.read_at(buffer, self.offset as u64)?;
        self.offset += read;
        Ok(read)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let mut state = self.state.lock();
        let node = state.node_by_inode_mut(self.inode)?;
        let data = match &mut node.kind {
            TmpNodeKind::File { data, .. } => data,
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => {
                return Err(FSError::NotAFile);
            }
        };
        let written = data.write_at(self.offset, buffer);
        self.offset = self.offset.saturating_add(written);
        Ok(written)
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let len = {
            let state = self.state.lock();
            let node = state.node_by_inode(self.inode)?;
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
        };
        if next < 0 {
            return Err(FSError::InvalidArguments);
        }
        self.offset = next as usize;
        Ok(self.offset)
    }

    fn truncate(&mut self, length: u64) -> FSResult<()> {
        let length = usize::try_from(length).map_err(|_| FSError::Other)?;
        let mut state = self.state.lock();
        let node = state.node_by_inode_mut(self.inode)?;
        let data = match &mut node.kind {
            TmpNodeKind::File { data, .. } => data,
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => {
                return Err(FSError::NotAFile);
            }
        };
        data.truncate(length);
        Ok(())
    }

    fn allocate(&mut self, mode: u32, offset: u64, len: u64) -> FSResult<()> {
        if mode != 0 {
            return Err(FSError::Other);
        }

        let offset = usize::try_from(offset).map_err(|_| FSError::Other)?;
        let len = usize::try_from(len).map_err(|_| FSError::Other)?;
        let end = offset.checked_add(len).ok_or(FSError::Other)?;
        let mut state = self.state.lock();
        let node = state.node_by_inode_mut(self.inode)?;
        let data = match &mut node.kind {
            TmpNodeKind::File { data, .. } => data,
            TmpNodeKind::Directory { .. } | TmpNodeKind::Symlink { .. } => {
                return Err(FSError::NotAFile);
            }
        };
        data.ensure_len(end);
        Ok(())
    }

    fn link_to(&self, new_path: &Path) -> FSResult<()> {
        self.state
            .lock()
            .link_inode(self.inode, &new_path.normalize().as_string())
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        let mut state = self.state.lock();
        if (mode & S_IFMT) != 0 {
            state.update_file_mode_by_inode(self.inode, mode)
        } else {
            state.update_file_mode_by_inode(self.inode, mode & 0o7777)
        }
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        self.state
            .lock()
            .update_owner_by_inode(self.inode, uid, gid)
    }

    fn set_times(&self, times: FileTimes) -> FSResult<()> {
        self.state.lock().update_times_by_inode(self.inode, times)
    }

    fn get_xattr(&self, name: &str) -> FSResult<Option<Vec<u8>>> {
        Ok(self.state.lock().xattr(self.inode, name))
    }

    fn set_xattr(&self, name: String, value: Vec<u8>, create: bool, replace: bool) -> FSResult<()> {
        self.state
            .lock()
            .set_xattr(self.inode, name, value, create, replace)
    }

    fn list_xattrs(&self) -> FSResult<Vec<String>> {
        self.state.lock().list_xattrs(self.inode)
    }

    fn remove_xattr(&self, name: &str) -> FSResult<()> {
        self.state.lock().remove_xattr(self.inode, name)
    }
}
