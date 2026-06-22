use alloc::{string::String, sync::Arc};

use crate::{
    filesystem::{block_device::BlockDevice, errors::FSError, info::LinuxStat, vfs_traits::Whence},
    impl_cast_function, impl_cast_function_non_trait,
    memory::{user_safe, utils::Mut},
    object::{
        Object,
        config::{ConfigurateRequest, LinuxHdGeometry},
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, Statable},
    },
};

const VIRTIO_BLK_MAJOR: u64 = 252;

pub struct BlockDeviceObject {
    name: String,
    minor: u64,
    device: Arc<dyn BlockDevice>,
    offset: Mut<usize>,
}

impl core::fmt::Debug for BlockDeviceObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BlockDeviceObject")
            .field("name", &self.name)
            .field("minor", &self.minor)
            .field("blocks", &self.device.total_blocks())
            .field("block_size", &self.device.block_size())
            .finish()
    }
}

impl BlockDeviceObject {
    pub fn new(name: String, minor: u64, device: Arc<dyn BlockDevice>) -> Self {
        Self {
            name,
            minor,
            device,
            offset: Mut::new(0),
        }
    }

    pub fn backing_device(&self) -> Arc<dyn BlockDevice> {
        self.device.clone()
    }

    pub fn rdev(&self) -> u64 {
        LinuxStat::linux_makedev(VIRTIO_BLK_MAJOR, self.minor)
    }

    pub fn read_at(&self, buffer: &mut [u8], offset: usize) -> ObjectResult<usize> {
        if offset >= self.device.total_bytes() {
            return Ok(0);
        }
        let len = buffer.len().min(self.device.total_bytes() - offset);
        self.device
            .read_by_bytes(offset, &mut buffer[..len])
            .map_err(FSError::from)
            .map_err(Into::into)
    }

    pub fn write_at(&self, buffer: &[u8], offset: usize) -> ObjectResult<usize> {
        if offset >= self.device.total_bytes() {
            return Ok(0);
        }
        let len = buffer.len().min(self.device.total_bytes() - offset);
        self.device
            .write_by_bytes(offset, &buffer[..len])
            .map_err(FSError::from)
            .map_err(Into::into)
    }

    pub fn read_from_cursor(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        let mut offset = self.offset.lock();
        let read = self.read_at(buffer, *offset)?;
        *offset += read;
        Ok(read)
    }

    pub fn write_to_cursor(&self, buffer: &[u8]) -> ObjectResult<usize> {
        let mut offset = self.offset.lock();
        let written = self.write_at(buffer, *offset)?;
        *offset += written;
        Ok(written)
    }
}

impl Object for BlockDeviceObject {
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("seekable", crate::object::traits::Seekable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("block_device", BlockDeviceObject);
}

impl Configuratable for BlockDeviceObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::BlockDiscard(range) | ConfigurateRequest::BlockZeroOut(range) => {
                let [offset, len] = user_safe::read(range).map_err(|_| ObjectError::BadAddress)?;
                self.zero_range(offset as usize, len as usize)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetGeometry(ptr) => {
                const HEADS: u64 = 255;
                const SECTORS: u64 = 63;
                let sectors = self.device.total_bytes() as u64 / 512;
                let cylinders = (sectors / (HEADS * SECTORS)).min(u16::MAX as u64);
                let geometry = LinuxHdGeometry {
                    heads: HEADS as u8,
                    sectors: SECTORS as u8,
                    cylinders: cylinders as u16,
                    start: 0,
                };
                user_safe::write(ptr, &geometry).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetAlignmentOffset(ptr) => {
                user_safe::write(ptr, &0).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetBlockSize(ptr) => {
                user_safe::write(ptr, &4096u64).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetDiskSequence(ptr) => {
                user_safe::write(ptr, &self.minor).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetIOOptimalSize(ptr) => {
                user_safe::write(ptr, &(self.device.block_size() as u32))
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetIOMinimumSize(ptr) => {
                user_safe::write(ptr, &(self.device.block_size() as u32))
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetPhysicalSectorSize(ptr) => {
                user_safe::write(ptr, &(self.device.block_size() as u32))
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetReadOnly(ptr) => {
                user_safe::write(ptr, &0).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockFlushBuffers => {
                self.device
                    .flush()
                    .map_err(FSError::from)
                    .map_err(ObjectError::from)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetSectorSize(ptr) => {
                user_safe::write(ptr, &512u32).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetSize(ptr) => {
                user_safe::write(ptr, &((self.device.total_bytes() / 512) as u64))
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetSize64(ptr) => {
                user_safe::write(ptr, &(self.device.total_bytes() as u64))
                    .map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetZoneCount(ptr)
            | ConfigurateRequest::BlockGetZoneSize(ptr) => {
                user_safe::write(ptr, &0).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            ConfigurateRequest::BlockGetDiscardZeroes(ptr) => {
                user_safe::write(ptr, &1).map_err(|_| ObjectError::BadAddress)?;
                Ok(0)
            }
            _ => Err(ObjectError::InvalidRequest),
        }
    }
}

impl BlockDeviceObject {
    fn zero_range(&self, offset: usize, len: usize) -> ObjectResult<()> {
        let end = offset
            .checked_add(len)
            .ok_or(ObjectError::InvalidArguments)?
            .min(self.device.total_bytes());
        if offset >= end {
            return Ok(());
        }

        let block_size = self.device.block_size().max(1);
        let chunk_size = block_size
            .saturating_mul(256)
            .min(1024 * 1024)
            .max(block_size);
        let zeroes = alloc::vec![0u8; chunk_size];
        let mut cursor = offset;
        while cursor < end {
            let chunk = (end - cursor).min(zeroes.len());
            self.write_at(&zeroes[..chunk], cursor)?;
            cursor += chunk;
        }
        Ok(())
    }
}

impl Statable for BlockDeviceObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::block_device_with_rdev(0o660, self.rdev())
    }
}

impl crate::object::traits::Seekable for BlockDeviceObject {
    fn seek(self: Arc<Self>, offset: i64, seek_type: Whence) -> ObjectResult<usize> {
        let mut current = self.offset.lock();
        let base = match seek_type {
            Whence::Start => 0,
            Whence::Current => *current as i64,
            Whence::End => self.device.total_bytes() as i64,
            Whence::Data | Whence::Hole => return Err(ObjectError::InvalidArguments),
        };
        let next = base
            .checked_add(offset)
            .ok_or(ObjectError::InvalidArguments)?;
        if next < 0 {
            return Err(ObjectError::InvalidArguments);
        }
        *current = next as usize;
        Ok(*current)
    }
}
