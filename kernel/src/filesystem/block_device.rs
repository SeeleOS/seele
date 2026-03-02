use core::cmp;

use fatfs::IoError;

#[derive(Debug)]
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
    fn read_block(&self, id: usize, buffer: &mut [u8]) -> BlockDeviceResult;
    fn write_block(&self, id: usize, buffer: &[u8]) -> BlockDeviceResult;

    fn total_bytes(&self) -> usize {
        self.total_blocks() * self.block_size()
    }

    fn read_by_bytes(&self, offset: usize, buffer: &mut [u8]) -> BlockDeviceResult {
        let block_id = offset / self.block_size();
        let offset_in_block = offset % self.block_size();

        let mut tmp_buffer = [0u8; 1024];

        self.read_block(block_id, &mut tmp_buffer)?;

        let available = self.block_size() - offset_in_block;
        let n = cmp::min(buffer.len(), available);

        buffer[..n].copy_from_slice(&tmp_buffer[offset_in_block..offset_in_block + n]);

        Ok(n)
    }
}
