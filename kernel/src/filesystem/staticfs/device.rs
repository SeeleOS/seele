use core::any::Any;

use crate::{
    filesystem::{
        errors::FSError,
        info::{FileLikeInfo, UnixPermission},
        staticfs::StaticDeviceNode,
        vfs::FSResult,
        vfs_traits::{File, FileLikeType, Whence},
    },
    object::{device::get_device_ref, misc::ObjectRef},
};

pub struct StaticDeviceHandle {
    node: &'static StaticDeviceNode,
    object: ObjectRef,
}

impl StaticDeviceHandle {
    pub fn new(node: &'static StaticDeviceNode) -> Self {
        let object = get_device_ref(node.device_name).expect("static device must resolve");
        Self { node, object }
    }

    pub fn object(&self) -> Result<ObjectRef, FSError> {
        Ok(self.object.clone())
    }
}

impl File for StaticDeviceHandle {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn info(&mut self) -> FSResult<FileLikeInfo> {
        Ok(FileLikeInfo::new(
            self.node.name.into(),
            0,
            UnixPermission(self.node.mode),
            FileLikeType::File,
        )
        .with_inode(self.node.inode))
    }

    fn read_at(&mut self, buffer: &mut [u8], _offset: u64) -> FSResult<usize> {
        self.read(buffer)
    }

    fn read(&mut self, buffer: &mut [u8]) -> FSResult<usize> {
        let object = self.object()?;
        let readable = object.as_readable().map_err(|_| FSError::Other)?;
        readable.read(buffer).map_err(|_| FSError::Other)
    }

    fn write(&mut self, buffer: &[u8]) -> FSResult<usize> {
        let object = self.object()?;
        let writable = object.as_writable().map_err(|_| FSError::Other)?;
        writable.write(buffer).map_err(|_| FSError::Other)
    }

    fn seek(&mut self, offset: i64, seek_type: Whence) -> FSResult<usize> {
        let object = self.object()?;
        let seekable = object.as_seekable().map_err(|_| FSError::Other)?;
        seekable.seek(offset, seek_type).map_err(|_| FSError::Other)
    }
}
