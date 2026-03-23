use alloc::slice;
use conquer_once::spin::OnceCell;
use spin::Mutex;

use crate::filesystem::block_device::{BlockDevice, BlockDeviceError};

#[derive(Debug)]
pub struct RamDisk(Mutex<&'static mut [u8]>);

pub static RAMDISK: OnceCell<RamDisk> = OnceCell::uninit();

pub fn init(addr: u64, len: u64) {
    unsafe {
        // The bootloader-provided ramdisk memory remains mapped for the
        // kernel lifetime, so storing a mutable slice here is valid.
        RAMDISK
            .get_or_init(|| RamDisk(Mutex::new(slice::from_raw_parts_mut(addr as *mut u8, len as usize))));
    }
}

impl BlockDevice for RamDisk {
    fn block_size(&self) -> usize {
        1024
    }

    fn read_single_block(
        &self,
        id: usize,
        buffer: &mut [u8],
    ) -> crate::filesystem::block_device::BlockDeviceResult {
        let start = id * self.block_size();
        let end = start + self.block_size();

        if buffer.len() < end - start {
            return Err(BlockDeviceError::BufferTooSmall);
        }

        let data = self.0.lock();
        if end > data.len() {
            return Err(BlockDeviceError::OutOfBounds);
        }

        buffer[..self.block_size()].copy_from_slice(&data[start..end]);

        Ok(end - start)
    }

    fn write_single_block(
        &self,
        id: usize,
        buffer: &[u8],
    ) -> crate::filesystem::block_device::BlockDeviceResult {
        let start = id * self.block_size();
        let end = start + self.block_size();

        if buffer.len() < end - start {
            return Err(BlockDeviceError::BufferTooSmall);
        }

        let mut data = self.0.lock();
        if end > data.len() {
            return Err(BlockDeviceError::OutOfBounds);
        }

        data[start..end].copy_from_slice(&buffer[..self.block_size()]);
        Ok(end - start)
    }

    fn total_blocks(&self) -> usize {
        self.0.lock().len() / self.block_size()
    }

    fn total_bytes(&self) -> usize {
        self.0.lock().len()
    }
}
