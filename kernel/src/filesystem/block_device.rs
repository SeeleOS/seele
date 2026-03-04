use core::cmp;

use alloc::vec;
use fatfs::IoError;

use crate::s_print;

pub mod initrd;

#[derive(Clone, Copy, Debug)]
pub enum BlockDeviceError {
    Readonly,
    OutOfBounds,
    BufferTooSmall,
    Other,
}

impl IoError for BlockDeviceError {
    fn is_interrupted(&self) -> bool {
        true
    }

    fn new_unexpected_eof_error() -> Self {
        Self::OutOfBounds
    }

    fn new_write_zero_error() -> Self {
        Self::Other
    }
}

pub type BlockDeviceResult = Result<usize, BlockDeviceError>;

pub trait BlockDevice: Send + Sync {
    fn total_blocks(&self) -> usize;
    fn block_size(&self) -> usize;
    fn read_single_block(&self, id: usize, buffer: &mut [u8]) -> BlockDeviceResult;
    fn write_single_block(&self, id: usize, buffer: &[u8]) -> BlockDeviceResult;

    fn read_blocks(&self, start: usize, buffer: &mut [u8]) -> BlockDeviceResult {
        let read_len = buffer.len() / self.block_size();

        for i in 0..read_len {
            let block = start + i;
            let byte = i * self.block_size();

            self.read_single_block(block, &mut buffer[byte..byte + self.block_size()])?;
        }

        Ok(buffer.len())
    }

    fn total_bytes(&self) -> usize {
        self.total_blocks() * self.block_size()
    }

    fn read_by_bytes(&self, offset: usize, buffer: &mut [u8]) -> BlockDeviceResult {
        let block_size = self.block_size();
        let starting_block = offset / block_size;
        let offset_in_block = offset % block_size;

        let tmpbuffer_size =
            (buffer.len() + offset_in_block + block_size - 1) / block_size * block_size;

        let mut tmp_buffer = alloc::vec![0u8; tmpbuffer_size];
        self.read_blocks(starting_block, &mut tmp_buffer)?;

        buffer.copy_from_slice(&tmp_buffer[offset_in_block..offset_in_block + buffer.len()]);

        Ok(buffer.len())
    }
}
