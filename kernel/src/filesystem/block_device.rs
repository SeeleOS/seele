use fatfs::IoError;

use crate::misc::error::AsSyscallError;
use seele_sys::errors::SyscallError;

pub mod initrd;

#[derive(Clone, Copy, Debug)]
pub enum BlockDeviceError {
    Readonly,
    OutOfBounds,
    BufferTooSmall,
    Other,
}

impl AsSyscallError for BlockDeviceError {
    fn as_syscall_error(&self) -> SyscallError {
        match self {
            Self::Readonly => SyscallError::ReadOnlyFileSystem,
            Self::OutOfBounds => SyscallError::InvalidArguments,
            Self::BufferTooSmall => SyscallError::InvalidArguments,
            Self::Other => SyscallError::IOError,
        }
    }
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
        // 向上取整：(len + size - 1) / size
        let read_len = (buffer.len() + self.block_size() - 1) / self.block_size();

        for i in 0..read_len {
            let block = start + i;
            let byte_start = i * self.block_size();
            let byte_end = (i + 1) * self.block_size();

            if byte_end <= buffer.len() {
                // 全块读取
                self.read_single_block(block, &mut buffer[byte_start..byte_end])?;
            } else {
                // 处理最后一个不满一整块的尾巴
                let mut temp = alloc::vec![0u8; self.block_size()];
                self.read_single_block(block, &mut temp)?;
                let buf_len = buffer.len();
                buffer[byte_start..].copy_from_slice(&temp[..buf_len - byte_start]);
            }
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

    fn write_by_bytes(&self, offset: usize, buffer: &[u8]) -> BlockDeviceResult {
        let block_size = self.block_size();
        let starting_block = offset / block_size;
        let offset_in_block = offset % block_size;
        let tmpbuffer_size =
            (buffer.len() + offset_in_block + block_size - 1) / block_size * block_size;

        let mut tmp_buffer = alloc::vec![0u8; tmpbuffer_size];
        // Read the existing data into the tmp buffer
        self.read_blocks(starting_block, &mut tmp_buffer)?;
        // Overwrite the tmp buffer with the actual data that we wanna write
        tmp_buffer[offset_in_block..offset_in_block + buffer.len()].copy_from_slice(buffer);

        // Write the blocks with the previous data and the actual data
        // NOTE: we need to read the original data of the block because we can only
        // write by block, and we dont wanna write nonsense into the block, so we
        // have to read it first.
        let write_len = tmp_buffer.len() / block_size;
        for i in 0..write_len {
            let block = starting_block + i;
            let byte_start = i * block_size;
            let byte_end = byte_start + block_size;
            self.write_single_block(block, &tmp_buffer[byte_start..byte_end])?;
        }

        Ok(buffer.len())
    }
}
