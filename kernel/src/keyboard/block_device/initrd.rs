use alloc::slice;
use conquer_once::spin::OnceCell;
use x86_64::VirtAddr;

use crate::filesystem::block_device::{BlockDevice, BlockDeviceError};

#[derive(Debug)]
pub struct RamDisk(&'static [u8]);

pub static RAMDISK: OnceCell<RamDisk> = OnceCell::uninit();

pub fn init(addr: u64, len: u64) {
    unsafe {
        RAMDISK.get_or_init(|| RamDisk(slice::from_raw_parts(addr as *const u8, len as usize)));
    }
}

impl BlockDevice for RamDisk {
    fn block_size(&self) -> usize {
        1024
    }

    fn read_block(
        &self,
        id: usize,
        buffer: &mut [u8],
    ) -> crate::filesystem::block_device::BlockDeviceResult {
        let start = id * self.block_size();
        let end = start + self.block_size();

        if buffer.len() < end - start {
            return Err(BlockDeviceError::BufferTooSmall);
        }

        if end > self.0.len() {
            return Err(BlockDeviceError::OutOfBounds);
        }

        buffer[..self.block_size()].copy_from_slice(&self.0[start..end]);

        Ok(())
    }

    fn write_block(
        &self,
        id: usize,
        buffer: &[u8],
    ) -> crate::filesystem::block_device::BlockDeviceResult {
        Err(BlockDeviceError::Readonly)
    }
}
