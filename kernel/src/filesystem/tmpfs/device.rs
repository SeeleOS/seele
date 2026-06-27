use core::any::Any;

use alloc::string::String;

use crate::{
    filesystem::{
        errors::FSError,
        info::{FileLikeInfo, FileTimes, UnixPermission},
        vfs::FSResult,
        vfs_traits::{File, FileLikeType, LinuxFileAttributes, Whence},
    },
    object::{device::get_device_ref_by_rdev, misc::ObjectRef},
};

use super::{TmpNodeKind, TmpfsStateRef};

pub(crate) struct TmpfsDeviceHandle {
    state: TmpfsStateRef,
    name: String,
    inode: u64,
    rdev: u64,
}

impl TmpfsDeviceHandle {
    pub(crate) fn new(state: TmpfsStateRef, name: String, inode: u64, rdev: u64) -> FSResult<Self> {
        Ok(Self {
            state,
            name,
            inode,
            rdev,
        })
    }

    pub(crate) fn object(&self) -> FSResult<ObjectRef> {
        get_device_ref_by_rdev(self.rdev).map_err(|_| FSError::InvalidArguments)
    }

    pub(crate) fn rdev(&self) -> u64 {
        self.rdev
    }
}

impl Drop for TmpfsDeviceHandle {
    fn drop(&mut self) {
        let _ = self.state.lock().release_inode(self.inode);
    }
}

impl File for TmpfsDeviceHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        let state = self.state.lock();
        let node = state.node_by_inode(self.inode)?;
        let TmpNodeKind::Device { mode, rdev } = node.kind else {
            return Err(FSError::NotAFile);
        };
        Ok(FileLikeInfo::new(
            self.name.clone(),
            0,
            UnixPermission(mode),
            FileLikeType::File,
        )
        .with_inode(self.inode)
        .with_owner(node.uid, node.gid)
        .with_nlink(node.link_count)
        .with_rdev(rdev)
        .with_times(node.times))
    }

    fn read_at(&mut self, buffer: &mut [u8], offset: u64) -> FSResult<usize> {
        let object = self.object()?;
        if let Ok(block_device) = object.clone().as_block_device() {
            return block_device
                .read_at(buffer, offset as usize)
                .map_err(|_| FSError::Other);
        }
        self.read(buffer)
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let object = self.object()?;
        if let Ok(block_device) = object.clone().as_block_device() {
            return block_device
                .read_from_cursor(buffer)
                .map_err(|_| FSError::Other);
        }
        let readable = object.as_readable().map_err(|_| FSError::Other)?;
        readable.read(buffer).map_err(|_| FSError::Other)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let object = self.object()?;
        if let Ok(block_device) = object.clone().as_block_device() {
            return block_device
                .write_to_cursor(buffer)
                .map_err(|_| FSError::Other);
        }
        let writable = object.as_writable().map_err(|_| FSError::Other)?;
        writable.write(buffer).map_err(|_| FSError::Other)
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let object = self.object()?;
        let seekable = object.as_seekable().map_err(|_| FSError::IllegalSeek)?;
        seekable.seek(offset, seek_type).map_err(|_| FSError::Other)
    }

    fn chmod(&self, mode: u32) -> FSResult<()> {
        self.state
            .lock()
            .update_file_mode_by_inode(self.inode, mode)
    }

    fn chown(&self, uid: u32, gid: u32) -> FSResult<()> {
        self.state
            .lock()
            .update_owner_by_inode(self.inode, uid, gid)
    }

    fn set_times(&self, times: FileTimes) -> FSResult<()> {
        self.state.lock().update_times_by_inode(self.inode, times)
    }

    fn linux_file_attributes(&self) -> FSResult<LinuxFileAttributes> {
        self.state.lock().file_attributes(self.inode)
    }

    fn set_linux_file_attributes(&self, attributes: LinuxFileAttributes) -> FSResult<()> {
        self.state
            .lock()
            .set_file_attributes(self.inode, attributes)
    }
}
