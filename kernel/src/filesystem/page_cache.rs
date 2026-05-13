use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::cmp::min;

use crate::memory::utils::Mut;
use lazy_static::lazy_static;

use crate::filesystem::vfs::{FSResult, WrappedFile};

const PAGE_SIZE: usize = 4096;
const FILE_CACHE_CLUSTER_PAGES: usize = 16;
const MAX_CACHED_PAGES: usize = 16_384;
const MAX_CACHED_CLUSTERS: usize = MAX_CACHED_PAGES / FILE_CACHE_CLUSTER_PAGES;

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
struct CachedClusterKey {
    file: FileCacheKey,
    cluster_index: u64,
}

#[derive(Clone, Debug)]
struct CachedCluster {
    data: Arc<Vec<u8>>,
    valid_len: usize,
}

#[derive(Clone, Debug)]
struct CachedClusterEntry {
    cluster: CachedCluster,
    referenced: bool,
}

#[derive(Clone, Debug)]
pub struct CachedPageRead {
    pub data: Arc<Vec<u8>>,
    pub valid_len: usize,
    pub was_hit: bool,
    pub lookup_cycles: u64,
    pub load_cycles: u64,
}

#[derive(Debug)]
struct PageCacheState {
    entries: BTreeMap<CachedClusterKey, CachedClusterEntry>,
    eviction_queue: VecDeque<CachedClusterKey>,
}

impl PageCacheState {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            eviction_queue: VecDeque::new(),
        }
    }

    fn touch(&mut self, key: CachedClusterKey) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.referenced = true;
        }
    }

    fn evict_one(&mut self) {
        while let Some(candidate) = self.eviction_queue.pop_front() {
            let Some(entry) = self.entries.get_mut(&candidate) else {
                continue;
            };

            if entry.referenced {
                entry.referenced = false;
                self.eviction_queue.push_back(candidate);
                continue;
            }

            self.entries.remove(&candidate);
            return;
        }
    }

    fn insert(&mut self, key: CachedClusterKey, cluster: CachedCluster) {
        if !self.entries.contains_key(&key) {
            while self.entries.len() >= MAX_CACHED_CLUSTERS {
                let before = self.entries.len();
                self.evict_one();
                if self.entries.len() == before {
                    break;
                }
            }

            self.eviction_queue.push_back(key);
        }

        self.entries.insert(
            key,
            CachedClusterEntry {
                cluster,
                referenced: true,
            },
        );
    }
}

lazy_static! {
    static ref PAGE_CACHE: Mut<PageCacheState> = Mut::new(PageCacheState::new());
}

fn cluster_len() -> usize {
    PAGE_SIZE * FILE_CACHE_CLUSTER_PAGES
}

fn load_cluster(
    file: &WrappedFile,
    cluster_offset: u64,
    file_size: u64,
) -> FSResult<CachedCluster> {
    let mut data = vec![0u8; cluster_len()];
    let remaining = file_size.saturating_sub(cluster_offset);
    let target_len = min(cluster_len(), remaining as usize);
    let mut valid_len = 0;

    if target_len != 0 {
        let mut file = file.lock();
        while valid_len < target_len {
            let read = file.read_at(
                &mut data[valid_len..target_len],
                cluster_offset + valid_len as u64,
            )?;
            if read == 0 {
                break;
            }
            valid_len += read;
        }
    }

    Ok(CachedCluster {
        data: Arc::new(data),
        valid_len,
    })
}

#[derive(Clone, Debug)]
pub struct CachedClusterRead {
    pub data: Arc<Vec<u8>>,
    pub valid_len: usize,
    pub was_hit: bool,
    pub lookup_cycles: u64,
    pub load_cycles: u64,
}

fn cluster_index_for_page(page_index: u64) -> u64 {
    page_index / FILE_CACHE_CLUSTER_PAGES as u64
}

fn page_offset_in_cluster(page_index: u64) -> usize {
    (page_index as usize % FILE_CACHE_CLUSTER_PAGES) * PAGE_SIZE
}

fn get_or_load_cluster(
    file: &WrappedFile,
    file_key: FileCacheKey,
    page_index: u64,
    file_size: u64,
) -> FSResult<CachedClusterRead> {
    let key = CachedClusterKey {
        file: file_key,
        cluster_index: cluster_index_for_page(page_index),
    };
    let lookup_start = crate::misc::profile::scope_start();

    {
        let mut state = PAGE_CACHE.lock();
        if let Some(cluster) = state.entries.get(&key).map(|entry| entry.cluster.clone()) {
            state.touch(key);
            return Ok(CachedClusterRead {
                data: cluster.data,
                valid_len: cluster.valid_len,
                was_hit: true,
                lookup_cycles: crate::misc::profile::scope_start().saturating_sub(lookup_start),
                load_cycles: 0,
            });
        }
    }

    let load_start = crate::misc::profile::scope_start();
    let cluster = load_cluster(
        file,
        cluster_index_for_page(page_index) * cluster_len() as u64,
        file_size,
    )?;
    let load_cycles = crate::misc::profile::scope_start().saturating_sub(load_start);

    let mut state = PAGE_CACHE.lock();
    if let Some(existing) = state.entries.get(&key).map(|entry| entry.cluster.clone()) {
        state.touch(key);
        return Ok(CachedClusterRead {
            data: existing.data,
            valid_len: existing.valid_len,
            was_hit: true,
            lookup_cycles: crate::misc::profile::scope_start().saturating_sub(lookup_start),
            load_cycles: 0,
        });
    }
    state.insert(key, cluster.clone());
    Ok(CachedClusterRead {
        data: cluster.data,
        valid_len: cluster.valid_len,
        was_hit: false,
        lookup_cycles: crate::misc::profile::scope_start().saturating_sub(lookup_start),
        load_cycles,
    })
}

fn read_page_from_cluster(cluster: &CachedClusterRead, page_index: u64) -> CachedPageRead {
    let page_offset = page_offset_in_cluster(page_index);
    let available = cluster.valid_len.saturating_sub(page_offset);
    let valid_len = min(PAGE_SIZE, available);
    CachedPageRead {
        data: cluster.data.clone(),
        valid_len,
        was_hit: cluster.was_hit,
        lookup_cycles: cluster.lookup_cycles,
        load_cycles: cluster.load_cycles,
    }
}

pub fn read_page(
    file: &WrappedFile,
    identity: FileCacheIdentity,
    page_index: u64,
) -> FSResult<CachedPageRead> {
    let cluster = get_or_load_cluster(file, identity.file, page_index, identity.size as u64)?;
    Ok(read_page_from_cluster(&cluster, page_index))
}

pub fn read_cluster(
    file: &WrappedFile,
    identity: FileCacheIdentity,
    page_index: u64,
) -> FSResult<CachedClusterRead> {
    get_or_load_cluster(file, identity.file, page_index, identity.size as u64)
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
}

#[cfg(test)]
pub fn reset_for_test() {
    *PAGE_CACHE.lock() = PageCacheState::new();
}

#[cfg(test)]
pub fn insert_for_test(file: FileCacheKey, page_index: u64, fill_byte: u8, referenced: bool) {
    let key = CachedClusterKey {
        file,
        cluster_index: cluster_index_for_page(page_index),
    };
    let cluster = CachedCluster {
        data: Arc::new(vec![fill_byte; cluster_len()]),
        valid_len: cluster_len(),
    };
    let mut state = PAGE_CACHE.lock();
    state.insert(key, cluster);
    if let Some(entry) = state.entries.get_mut(&key) {
        entry.referenced = referenced;
    }
}

#[cfg(test)]
pub fn contains_for_test(file: FileCacheKey, page_index: u64) -> bool {
    let key = CachedClusterKey {
        file,
        cluster_index: cluster_index_for_page(page_index),
    };
    PAGE_CACHE.lock().entries.contains_key(&key)
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
        let page = read_page(file, identity, page_index)?;
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
