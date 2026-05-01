use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::cmp::min;

use lazy_static::lazy_static;
use spin::Mutex;

use crate::filesystem::vfs::{FSResult, WrappedFile};

const PAGE_SIZE: usize = 4096;
const MAX_CACHED_PAGES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileCacheKey {
    pub device_id: u64,
    pub inode: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct FileCacheIdentity {
    pub file: FileCacheKey,
    pub size: usize,
}

impl FileCacheIdentity {
    pub fn new(device_id: u64, inode: u64, size: usize) -> Self {
        Self {
            file: FileCacheKey { device_id, inode },
            size,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CachedPageKey {
    file: FileCacheKey,
    page_index: u64,
}

#[derive(Clone, Debug)]
struct CachedPage {
    data: Arc<Vec<u8>>,
    valid_len: usize,
}

#[derive(Debug)]
struct PageCacheState {
    entries: BTreeMap<CachedPageKey, CachedPage>,
    lru: VecDeque<CachedPageKey>,
}

impl PageCacheState {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            lru: VecDeque::new(),
        }
    }

    fn touch(&mut self, key: CachedPageKey) {
        self.lru.retain(|candidate| *candidate != key);
        self.lru.push_back(key);
    }

    fn insert(&mut self, key: CachedPageKey, page: CachedPage) {
        if !self.entries.contains_key(&key) {
            while self.entries.len() >= MAX_CACHED_PAGES {
                let Some(evicted) = self.lru.pop_front() else {
                    break;
                };
                self.entries.remove(&evicted);
            }
        }

        self.entries.insert(key, page);
        self.touch(key);
    }
}

lazy_static! {
    static ref PAGE_CACHE: Mutex<PageCacheState> = Mutex::new(PageCacheState::new());
}

fn load_page(file: &WrappedFile, page_offset: u64, file_size: u64) -> FSResult<CachedPage> {
    let mut data = vec![0u8; PAGE_SIZE];
    let remaining = file_size.saturating_sub(page_offset);
    let target_len = min(PAGE_SIZE, remaining as usize);
    let mut valid_len = 0;

    if target_len != 0 {
        let mut file = file.lock();
        while valid_len < target_len {
            let read = file.read_at(
                &mut data[valid_len..target_len],
                page_offset + valid_len as u64,
            )?;
            if read == 0 {
                break;
            }
            valid_len += read;
        }
    }

    Ok(CachedPage {
        data: Arc::new(data),
        valid_len,
    })
}

fn get_or_load_page(
    file: &WrappedFile,
    file_key: FileCacheKey,
    page_index: u64,
    file_size: u64,
) -> FSResult<CachedPage> {
    let key = CachedPageKey {
        file: file_key,
        page_index,
    };

    {
        let mut state = PAGE_CACHE.lock();
        if let Some(page) = state.entries.get(&key).cloned() {
            state.touch(key);
            return Ok(page);
        }
    }

    let page = load_page(file, page_index * PAGE_SIZE as u64, file_size)?;

    let mut state = PAGE_CACHE.lock();
    if let Some(existing) = state.entries.get(&key).cloned() {
        state.touch(key);
        return Ok(existing);
    }
    state.insert(key, page.clone());
    Ok(page)
}

pub fn invalidate_file(file: FileCacheKey) {
    let mut state = PAGE_CACHE.lock();
    let keys = state
        .entries
        .keys()
        .copied()
        .filter(|key| key.file == file)
        .collect::<Vec<_>>();

    for key in keys {
        state.entries.remove(&key);
    }
    state.lru.retain(|key| key.file != file);
}

pub fn read(
    file: &WrappedFile,
    identity: FileCacheIdentity,
    buffer: &mut [u8],
    offset: u64,
) -> FSResult<usize> {
    if buffer.is_empty() {
        return Ok(0);
    }

    let file_size = identity.size as u64;
    if offset >= file_size {
        return Ok(0);
    }

    let mut total = 0;
    let mut current_offset = offset;

    while total < buffer.len() && current_offset < file_size {
        let page_index = current_offset / PAGE_SIZE as u64;
        let page_offset = (current_offset % PAGE_SIZE as u64) as usize;
        let page = get_or_load_page(file, identity.file, page_index, file_size)?;
        let available = page.valid_len.saturating_sub(page_offset);
        if available == 0 {
            break;
        }

        let copy_len = min(available, buffer.len() - total);
        buffer[total..total + copy_len]
            .copy_from_slice(&page.data[page_offset..page_offset + copy_len]);
        total += copy_len;
        current_offset += copy_len as u64;
    }

    Ok(total)
}
