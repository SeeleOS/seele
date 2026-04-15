use fatfs::IoError;

use crate::misc::error::AsSyscallError;
use crate::systemcall::utils::SyscallError;

pub mod cache;
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
    fn flush(&self) -> Result<(), BlockDeviceError> {
        Ok(())
    }

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
        if buffer.is_empty() {
            return Ok(0);
        }

        let block_size = self.block_size();
        let starting_block = offset / block_size;
        let offset_in_block = offset % block_size;
        let ending_offset = offset + buffer.len();
        let ending_block = ending_offset.div_ceil(block_size);

        if offset_in_block == 0 && buffer.len().is_multiple_of(block_size) {
            self.read_blocks(starting_block, buffer)?;
            return Ok(buffer.len());
        }

        let mut copied = 0;

        if offset_in_block != 0 {
            let mut temp = alloc::vec![0u8; block_size];
            self.read_single_block(starting_block, &mut temp)?;
            let head_len = core::cmp::min(block_size - offset_in_block, buffer.len());
            buffer[..head_len].copy_from_slice(&temp[offset_in_block..offset_in_block + head_len]);
            copied += head_len;
        }

        let full_blocks_start = starting_block + usize::from(offset_in_block != 0);
        let full_blocks_end = ending_block - usize::from(!ending_offset.is_multiple_of(block_size));
        let full_blocks = full_blocks_end.saturating_sub(full_blocks_start);
        if full_blocks != 0 {
            let full_bytes = full_blocks * block_size;
            self.read_blocks(
                full_blocks_start,
                &mut buffer[copied..copied + full_bytes],
            )?;
            copied += full_bytes;
        }

        if copied < buffer.len() {
            let mut temp = alloc::vec![0u8; block_size];
            self.read_single_block(ending_block - 1, &mut temp)?;
            let tail_len = buffer.len() - copied;
            buffer[copied..].copy_from_slice(&temp[..tail_len]);
        }

        Ok(buffer.len())
    }

    fn write_by_bytes(&self, offset: usize, buffer: &[u8]) -> BlockDeviceResult {
        if buffer.is_empty() {
            return Ok(0);
        }

        let block_size = self.block_size();
        let starting_block = offset / block_size;
        let offset_in_block = offset % block_size;
        let ending_offset = offset + buffer.len();
        let ending_block = ending_offset.div_ceil(block_size);

        if offset_in_block == 0 && buffer.len().is_multiple_of(block_size) {
            self.write_blocks(starting_block, buffer)?;
            return Ok(buffer.len());
        }

        let mut written = 0;

        if offset_in_block != 0 {
            let mut temp = alloc::vec![0u8; block_size];
            self.read_single_block(starting_block, &mut temp)?;
            let head_len = core::cmp::min(block_size - offset_in_block, buffer.len());
            temp[offset_in_block..offset_in_block + head_len].copy_from_slice(&buffer[..head_len]);
            self.write_single_block(starting_block, &temp)?;
            written += head_len;
        }

        let full_blocks_start = starting_block + usize::from(offset_in_block != 0);
        let full_blocks_end = ending_block - usize::from(!ending_offset.is_multiple_of(block_size));
        let full_blocks = full_blocks_end.saturating_sub(full_blocks_start);
        if full_blocks != 0 {
            let full_bytes = full_blocks * block_size;
            self.write_blocks(full_blocks_start, &buffer[written..written + full_bytes])?;
            written += full_bytes;
        }

        if written < buffer.len() {
            let last_block = ending_block - 1;
            let mut temp = alloc::vec![0u8; block_size];
            self.read_single_block(last_block, &mut temp)?;
            temp[..buffer.len() - written].copy_from_slice(&buffer[written..]);
            self.write_single_block(last_block, &temp)?;
        }

        Ok(buffer.len())
    }

    fn write_blocks(&self, start: usize, buffer: &[u8]) -> BlockDeviceResult {
        let block_size = self.block_size();
        if !buffer.len().is_multiple_of(block_size) {
            return Err(BlockDeviceError::BufferTooSmall);
        }

        let write_len = buffer.len() / block_size;
        for i in 0..write_len {
            let block = start + i;
            let byte_start = i * block_size;
            let byte_end = byte_start + block_size;
            self.write_single_block(block, &buffer[byte_start..byte_end])?;
        }

        Ok(buffer.len())
    }
}
