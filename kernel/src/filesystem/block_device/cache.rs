use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec,
    vec::Vec,
};
use spin::Mutex;

use crate::filesystem::block_device::{BlockDevice, BlockDeviceError, BlockDeviceResult};

const DEFAULT_CACHE_ENTRIES: usize = 4_096;
const DEFAULT_READAHEAD_BLOCKS: usize = 32;

#[derive(Debug)]
struct CacheEntry {
    data: Vec<u8>,
    dirty: bool,
}

#[derive(Debug)]
struct CacheState {
    entries: BTreeMap<usize, CacheEntry>,
    lru: VecDeque<usize>,
}

impl CacheState {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            lru: VecDeque::new(),
        }
    }

    fn touch(&mut self, block_id: usize) {
        self.lru.push_back(block_id);
    }
}

pub struct CachedBlockDevice {
    inner: Arc<dyn BlockDevice>,
    max_entries: usize,
    readahead_blocks: usize,
    state: Mutex<CacheState>,
}

impl CachedBlockDevice {
    pub fn new(inner: Arc<dyn BlockDevice>) -> Self {
        Self::with_capacity(inner, DEFAULT_CACHE_ENTRIES)
    }

    pub fn with_capacity(inner: Arc<dyn BlockDevice>, max_entries: usize) -> Self {
        Self {
            inner,
            max_entries: max_entries.max(1),
            readahead_blocks: DEFAULT_READAHEAD_BLOCKS,
            state: Mutex::new(CacheState::new()),
        }
    }

    fn evict_if_needed(&self, state: &mut CacheState) -> Result<(), BlockDeviceError> {
        while state.entries.len() >= self.max_entries {
            let Some(block_id) = state.lru.pop_front() else {
                break;
            };
            let Some(entry) = state.entries.remove(&block_id) else {
                continue;
            };
            if entry.dirty {
                self.inner.write_single_block(block_id, &entry.data)?;
            }
        }
        Ok(())
    }

    fn write_back_dirty(&self, state: &mut CacheState) -> Result<(), BlockDeviceError> {
        let block_size = self.block_size();
        let mut dirty_blocks = state
            .entries
            .iter()
            .filter_map(|(&block_id, entry)| entry.dirty.then_some(block_id))
            .collect::<Vec<_>>();
        dirty_blocks.sort_unstable();

        let mut index = 0;
        while index < dirty_blocks.len() {
            let start_block = dirty_blocks[index];
            let mut end = index + 1;
            while end < dirty_blocks.len() && dirty_blocks[end] == dirty_blocks[end - 1] + 1 {
                end += 1;
            }

            let block_count = end - index;
            let mut buffer = Vec::with_capacity(block_count * block_size);
            for &block_id in &dirty_blocks[index..end] {
                buffer.extend_from_slice(&state.entries.get(&block_id).unwrap().data);
            }

            self.inner.write_blocks(start_block, &buffer)?;

            for &block_id in &dirty_blocks[index..end] {
                if let Some(entry) = state.entries.get_mut(&block_id) {
                    entry.dirty = false;
                }
            }

            index = end;
        }

        Ok(())
    }
}

impl BlockDevice for CachedBlockDevice {
    fn total_blocks(&self) -> usize {
        self.inner.total_blocks()
    }

    fn block_size(&self) -> usize {
        self.inner.block_size()
    }

    fn read_single_block(&self, id: usize, buffer: &mut [u8]) -> BlockDeviceResult {
        let block_size = self.block_size();
        if buffer.len() < block_size {
            return Err(BlockDeviceError::BufferTooSmall);
        }

        {
            let state = self.state.lock();
            if let Some(entry) = state.entries.get(&id) {
                buffer[..block_size].copy_from_slice(&entry.data);
                return Ok(block_size);
            }
        }

        let mut data = alloc::vec![0u8; block_size];
        self.inner.read_single_block(id, &mut data)?;

        let mut state = self.state.lock();
        self.evict_if_needed(&mut state)?;
        buffer[..block_size].copy_from_slice(&data);
        state.entries.insert(id, CacheEntry { data, dirty: false });
        state.touch(id);

        Ok(block_size)
    }

    fn read_blocks(&self, start: usize, buffer: &mut [u8]) -> BlockDeviceResult {
        let block_size = self.block_size();
        if !buffer.len().is_multiple_of(block_size) {
            return Err(BlockDeviceError::BufferTooSmall);
        }

        let total_blocks = buffer.len() / block_size;
        if start + total_blocks > self.total_blocks() {
            return Err(BlockDeviceError::OutOfBounds);
        }

        let mut state = self.state.lock();
        let mut index = 0;
        while index < total_blocks {
            let block_id = start + index;

            if let Some(entry) = state.entries.get(&block_id) {
                let byte_start = index * block_size;
                buffer[byte_start..byte_start + block_size].copy_from_slice(&entry.data);
                index += 1;
                continue;
            }

            let miss_start = index;
            let mut miss_end = index + 1;
            while miss_end < total_blocks && !state.entries.contains_key(&(start + miss_end)) {
                miss_end += 1;
            }

            let readahead_end = core::cmp::min(total_blocks, miss_end + self.readahead_blocks);
            let fetch_blocks = readahead_end - miss_start;
            let mut temp = vec![0u8; fetch_blocks * block_size];
            drop(state);
            self.inner.read_blocks(start + miss_start, &mut temp)?;
            state = self.state.lock();

            for block_offset in miss_start..readahead_end {
                self.evict_if_needed(&mut state)?;
                let cache_block_id = start + block_offset;
                let temp_offset = (block_offset - miss_start) * block_size;
                let data = temp[temp_offset..temp_offset + block_size].to_vec();
                if block_offset < miss_end {
                    let byte_start = block_offset * block_size;
                    buffer[byte_start..byte_start + block_size].copy_from_slice(&data);
                }
                state.entries.insert(
                    cache_block_id,
                    CacheEntry {
                        data,
                        dirty: false,
                    },
                );
                state.touch(cache_block_id);
            }

            index = miss_end;
        }

        Ok(buffer.len())
    }

    fn write_single_block(&self, id: usize, buffer: &[u8]) -> BlockDeviceResult {
        let block_size = self.block_size();
        if buffer.len() < block_size {
            return Err(BlockDeviceError::BufferTooSmall);
        }

        let mut state = self.state.lock();
        self.evict_if_needed(&mut state)?;
        let data = buffer[..block_size].to_vec();
        state.entries.insert(id, CacheEntry { data, dirty: true });
        state.touch(id);
        Ok(block_size)
    }

    fn write_blocks(&self, start: usize, buffer: &[u8]) -> BlockDeviceResult {
        let block_size = self.block_size();
        if !buffer.len().is_multiple_of(block_size) {
            return Err(BlockDeviceError::BufferTooSmall);
        }

        let total_blocks = buffer.len() / block_size;
        if start + total_blocks > self.total_blocks() {
            return Err(BlockDeviceError::OutOfBounds);
        }

        let mut state = self.state.lock();
        for index in 0..total_blocks {
            self.evict_if_needed(&mut state)?;
            let block_id = start + index;
            let byte_start = index * block_size;
            state.entries.insert(
                block_id,
                CacheEntry {
                    data: buffer[byte_start..byte_start + block_size].to_vec(),
                    dirty: true,
                },
            );
            state.touch(block_id);
        }

        Ok(buffer.len())
    }

    fn flush(&self) -> Result<(), BlockDeviceError> {
        let mut state = self.state.lock();
        self.write_back_dirty(&mut state)?;
        self.inner.flush()
    }
}
