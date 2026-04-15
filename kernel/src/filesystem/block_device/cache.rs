use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;

use crate::filesystem::block_device::{BlockDevice, BlockDeviceError, BlockDeviceResult};

const DEFAULT_CACHE_ENTRIES: usize = 256;

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
        if let Some(index) = self.lru.iter().position(|&id| id == block_id) {
            self.lru.remove(index);
        }
        self.lru.push_back(block_id);
    }
}

pub struct CachedBlockDevice {
    inner: Arc<dyn BlockDevice>,
    max_entries: usize,
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
            let mut state = self.state.lock();
            if let Some(entry) = state.entries.get(&id) {
                buffer[..block_size].copy_from_slice(&entry.data);
                state.touch(id);
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

    fn flush(&self) -> Result<(), BlockDeviceError> {
        let mut state = self.state.lock();
        for (&block_id, entry) in state.entries.iter_mut() {
            if entry.dirty {
                self.inner.write_single_block(block_id, &entry.data)?;
                entry.dirty = false;
            }
        }
        self.inner.flush()
    }
}
