use crate::memory::utils::Mut;
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::ptr;
use x86_64::structures::paging::{FrameAllocator, Page, PageTableFlags, PhysFrame, Size4KiB};

use crate::{
    filesystem::{
        object::FileLikeObject,
        page_cache::{self, FileCacheIdentity, FileCacheKey},
        vfs::WrappedFile,
    },
    memory::{
        addrspace::{
            AddrSpace, AllocResult,
            cow::increase_ref,
            mem_area::{Data, MemoryArea},
        },
        paging::FRAME_ALLOCATOR,
        utils::apply_offset,
    },
    misc::profile::{self, HotSyscallPhase},
    misc::stack_builder::StackBuilder,
};
use lazy_static::lazy_static;

#[cfg(test)]
use alloc::vec;

const FILE_LAZY_CLUSTER_PAGES: u64 = 16;

#[derive(Clone, Copy, Debug, Default)]
pub struct FileLazyFaultStats {
    pub cluster_pages_loaded: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_lookup_cycles: u64,
    pub cache_load_cycles: u64,
    pub map_cycles: u64,
    pub copy_cycles: u64,
}

#[derive(Clone, Copy, Debug)]
struct SharedFileFrameLoad {
    frame: PhysFrame,
    was_hit: bool,
    lookup_cycles: u64,
    load_cycles: u64,
}

impl Default for SharedFileFrameLoad {
    fn default() -> Self {
        Self {
            frame: PhysFrame::from_start_address(x86_64::PhysAddr::new(0)).unwrap(),
            was_hit: false,
            lookup_cycles: 0,
            load_cycles: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FilePageCacheKey {
    device_id: u64,
    inode: u64,
    offset: u64,
}

lazy_static! {
    static ref SHARED_FILE_PAGE_CACHE: Mut<BTreeMap<FilePageCacheKey, PhysFrame>> =
        Mut::new(BTreeMap::new());
}

#[derive(Clone)]
struct ResolvedFileCache {
    wrapped: WrappedFile,
    identity: FileCacheIdentity,
}

impl AddrSpace {
    fn map_existing_frame(
        &mut self,
        page: Page<Size4KiB>,
        frame: PhysFrame,
        flags: PageTableFlags,
        refcount_frame: bool,
    ) -> PhysFrame {
        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();

        unsafe {
            self.page_table
                .map_to(page, frame, flags, &mut *frame_allocator)
                .unwrap()
                .flush();
        }

        if refcount_frame {
            increase_ref(frame);
        }
        frame
    }

    fn alloc_zeroed_frame() -> PhysFrame {
        let frame = FRAME_ALLOCATOR
            .get()
            .unwrap()
            .lock()
            .allocate_frame()
            .expect("memory full;");
        unsafe {
            core::ptr::write_bytes(
                apply_offset(frame.start_address().as_u64()) as *mut u8,
                0,
                4096,
            );
        }
        frame
    }

    fn readonly_file_page_cache_key(file: FileCacheKey, offset: u64) -> Option<FilePageCacheKey> {
        Some(FilePageCacheKey {
            device_id: file.device_id,
            inode: file.inode,
            offset,
        })
    }

    fn resolve_file_cache(file: &Arc<FileLikeObject>) -> Option<ResolvedFileCache> {
        let (wrapped, identity) = file.readonly_page_cache_file()?;
        Some(ResolvedFileCache { wrapped, identity })
    }

    fn get_or_load_shared_file_frame(
        resolved: &ResolvedFileCache,
        offset: u64,
        read_len: usize,
    ) -> Option<SharedFileFrameLoad> {
        let lookup_start = profile::scope_start();
        let key = Self::readonly_file_page_cache_key(resolved.identity.file, offset)?;
        let mut cache = SHARED_FILE_PAGE_CACHE.lock();
        if let Some(frame) = cache.get(&key).copied() {
            return Some(SharedFileFrameLoad {
                frame,
                was_hit: true,
                lookup_cycles: profile::scope_start().saturating_sub(lookup_start),
                ..Default::default()
            });
        }

        let frame = Self::alloc_zeroed_frame();
        if read_len != 0 {
            let cluster =
                page_cache::read_cluster(&resolved.wrapped, resolved.identity, offset / 4096)
                    .ok()?;
            let cluster_offset =
                ((offset as usize) % (4096 * FILE_LAZY_CLUSTER_PAGES as usize)) & !(4096 - 1);
            let available = cluster.valid_len.saturating_sub(cluster_offset);
            let copy_len = core::cmp::min(read_len, available);
            unsafe {
                ptr::copy_nonoverlapping(
                    cluster.data.as_ptr().add(cluster_offset),
                    apply_offset(frame.start_address().as_u64()) as *mut u8,
                    copy_len,
                );
            }
        }

        increase_ref(frame);
        cache.insert(key, frame);
        Some(SharedFileFrameLoad {
            frame,
            was_hit: false,
            lookup_cycles: profile::scope_start().saturating_sub(lookup_start),
            load_cycles: 0,
        })
    }

    fn copy_cached_file_page(
        wrapped: &WrappedFile,
        identity: FileCacheIdentity,
        file_offset: u64,
        read_len: usize,
        dst_ptr: *mut u8,
    ) -> Option<FileLazyFaultStats> {
        if read_len == 0 {
            return Some(FileLazyFaultStats {
                cluster_pages_loaded: 1,
                cache_hits: 1,
                ..Default::default()
            });
        }

        let mut stats = FileLazyFaultStats {
            cluster_pages_loaded: 1,
            ..Default::default()
        };
        let copy_start = profile::scope_start();
        let page_index = file_offset / 4096;
        let cached = page_cache::read_cluster(wrapped, identity, page_index).ok()?;
        stats.cache_lookup_cycles = cached.lookup_cycles;
        stats.cache_load_cycles = cached.load_cycles;
        if cached.was_hit {
            stats.cache_hits = 1;
        } else {
            stats.cache_misses = 1;
        }

        let cluster_offset = (file_offset as usize) % (4096 * FILE_LAZY_CLUSTER_PAGES as usize);
        let available = cached.valid_len.saturating_sub(cluster_offset);
        let copy_len = core::cmp::min(available, read_len);
        if copy_len != 0 {
            let memcpy_start = profile::scope_start();
            unsafe {
                ptr::copy_nonoverlapping(cached.data[cluster_offset..].as_ptr(), dst_ptr, copy_len);
            }
            profile::record_hot_syscall_phase(
                HotSyscallPhase::PfFileCopyMemcpy,
                profile::scope_start().saturating_sub(memcpy_start),
            );
        }

        let zero_fill_len = read_len.saturating_sub(copy_len);
        if zero_fill_len != 0 {
            let zero_fill_start = profile::scope_start();
            unsafe {
                ptr::write_bytes(dst_ptr.add(copy_len), 0, zero_fill_len);
            }
            profile::record_hot_syscall_phase(
                HotSyscallPhase::PfFileCopyZeroFill,
                profile::scope_start().saturating_sub(zero_fill_start),
            );
        }

        stats.copy_cycles = profile::scope_start().saturating_sub(copy_start);
        Some(stats)
    }

    #[cfg(test)]
    pub(crate) fn copy_cached_file_page_for_test(
        wrapped: &WrappedFile,
        identity: FileCacheIdentity,
        file_offset: u64,
        read_len: usize,
        buffer_len: usize,
    ) -> Option<(Vec<u8>, FileLazyFaultStats)> {
        let mut buffer = vec![0u8; buffer_len];
        let stats = Self::copy_cached_file_page(
            wrapped,
            identity,
            file_offset,
            read_len,
            buffer.as_mut_ptr(),
        )?;
        Some((buffer, stats))
    }

    pub fn apply_page(&mut self, page: Page<Size4KiB>, area: MemoryArea) -> PhysFrame {
        self.apply_page_cluster(page, area, 1).frame
    }

    pub fn apply_page_cluster(
        &mut self,
        page: Page<Size4KiB>,
        area: MemoryArea,
        cluster_pages: u64,
    ) -> AppliedPageCluster {
        match area.data {
            Data::Normal(_) => AppliedPageCluster {
                frame: self.alloc_map_zeroed_page(page, area, true).0,
                file_lazy_stats: FileLazyFaultStats::default(),
            },
            Data::File {
                offset,
                file_bytes,
                ref file,
                shared,
            } => {
                let resolved = Self::resolve_file_cache(file)
                    .expect("file-backed mapping missing readonly page-cache backend");
                // Only truly shared file mappings may reuse a cached physical
                // page. Private ELF PT_LOADs can legally map the same file page
                // through multiple segment views with different valid byte
                // ranges, so deduplicating them by file offset can zero or
                // truncate data that another view still needs.
                let use_shared_cache = shared;
                let page_index = (page.start_address().as_u64() - area.start.as_u64()) / 4096;
                let max_pages =
                    core::cmp::min(cluster_pages, area.pages().saturating_sub(page_index));
                let mut first_frame = None;
                let mut stats = FileLazyFaultStats::default();

                for i in 0..max_pages {
                    let current_page = page + i;
                    if self
                        .page_table
                        .translate_addr(current_page.start_address())
                        .is_some()
                    {
                        continue;
                    }

                    let page_offset = current_page.start_address().as_u64() - area.start.as_u64();
                    let read_len = core::cmp::min(4096, file_bytes.saturating_sub(page_offset));
                    let file_offset = offset + page_offset;

                    if let (true, Some(shared_frame)) = (
                        use_shared_cache,
                        Self::get_or_load_shared_file_frame(
                            &resolved,
                            file_offset,
                            read_len as usize,
                        ),
                    ) {
                        let shared_ref_start = profile::scope_start();
                        let map_start = profile::scope_start();
                        let frame = self.map_existing_frame(
                            current_page,
                            shared_frame.frame,
                            area.flags,
                            true,
                        );
                        stats.cache_lookup_cycles += shared_frame.lookup_cycles;
                        stats.cache_load_cycles += shared_frame.load_cycles;
                        if shared_frame.was_hit {
                            stats.cache_hits += 1;
                        } else {
                            stats.cache_misses += 1;
                        }
                        profile::record_hot_syscall_phase(
                            HotSyscallPhase::PfFileCopySharedRef,
                            profile::scope_start().saturating_sub(shared_ref_start),
                        );
                        stats.map_cycles += profile::scope_start().saturating_sub(map_start);
                        stats.cluster_pages_loaded += 1;
                        if first_frame.is_none() {
                            first_frame = Some(frame);
                        }
                        continue;
                    }

                    let map_start = profile::scope_start();
                    let (frame, write_addr) =
                        self.alloc_map_zeroed_page(current_page, area.clone(), read_len < 4096);
                    profile::record_hot_syscall_phase(
                        HotSyscallPhase::PfFileCopyFrameMap,
                        profile::scope_start().saturating_sub(map_start),
                    );
                    stats.map_cycles += profile::scope_start().saturating_sub(map_start);
                    stats.cluster_pages_loaded += 1;

                    if first_frame.is_none() {
                        first_frame = Some(frame);
                    }

                    let read_len = read_len as usize;
                    if read_len != 0 {
                        let cache_read_start = profile::scope_start();
                        let copy_stats = Self::copy_cached_file_page(
                            &resolved.wrapped,
                            resolved.identity,
                            file_offset,
                            read_len,
                            write_addr as *mut u8,
                        )
                        .expect("Failed to lazyload file-backed page through page cache");
                        profile::record_hot_syscall_phase(
                            HotSyscallPhase::PfFileCopyCacheRead,
                            profile::scope_start().saturating_sub(cache_read_start),
                        );
                        stats.cluster_pages_loaded += copy_stats.cluster_pages_loaded - 1;
                        stats.cache_hits += copy_stats.cache_hits;
                        stats.cache_misses += copy_stats.cache_misses;
                        stats.cache_lookup_cycles += copy_stats.cache_lookup_cycles;
                        stats.cache_load_cycles += copy_stats.cache_load_cycles;
                        stats.copy_cycles += copy_stats.copy_cycles;
                    } else {
                        stats.cache_hits += 1;
                    }
                }

                AppliedPageCluster {
                    frame: first_frame.unwrap_or_else(|| {
                        self.page_table
                            .translate_page(page)
                            .expect("file-backed cluster fault target page still unmapped")
                    }),
                    file_lazy_stats: stats,
                }
            }
            Data::Shared {
                ref frames,
                flags: shared_flags,
            } => {
                let page_index = (page.start_address().as_u64() - area.start.as_u64()) / 4096;
                let frame = frames.get(page_index as usize);
                AppliedPageCluster {
                    frame: self.map_existing_frame(page, frame, area.flags | shared_flags, true),
                    file_lazy_stats: FileLazyFaultStats::default(),
                }
            }
        }
    }

    pub fn apply_area(&mut self, area: MemoryArea) -> AllocResult {
        log::trace!(
            "addrspace: apply_region start {:#x} pages {}",
            area.start.as_u64(),
            area.pages()
        );
        let start = area.start_page();
        let pages = area.pages();

        let mut page_write_bases = Vec::with_capacity(pages as usize);

        match area.data {
            Data::File { .. } => {
                for i in 0..pages {
                    let page = start + i;
                    let frame = self.apply_page_cluster(page, area.clone(), 1).frame;
                    page_write_bases.push(apply_offset(frame.start_address().as_u64()));
                }
            }
            Data::Normal(_) => {
                for i in 0..pages {
                    let page = start + i;
                    let frame = self.alloc_map_zeroed_page(page, area.clone(), true).0;
                    page_write_bases.push(apply_offset(frame.start_address().as_u64()));
                }
            }
            Data::Shared { .. } => {
                for i in 0..pages {
                    let page = start + i;
                    let frame = self.apply_page(page, area.clone());
                    page_write_bases.push(apply_offset(frame.start_address().as_u64()));
                }
            }
        }

        let start_addr = start.start_address();
        let end_addr = (start + pages).start_address();

        (
            start_addr,
            StackBuilder::new(end_addr.as_u64(), page_write_bases),
        )
    }

    pub fn file_lazy_cluster_pages() -> u64 {
        FILE_LAZY_CLUSTER_PAGES
    }

    fn alloc_map_zeroed_page(
        &mut self,
        page: Page<Size4KiB>,
        area: MemoryArea,
        zero_page: bool,
    ) -> (PhysFrame, u64) {
        let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();
        let frame = frame_allocator.allocate_frame().expect("memory full;");

        unsafe {
            self.page_table
                .map_to(page, frame, area.flags, &mut *frame_allocator)
                .unwrap()
                .flush();
        };

        let write_addr = apply_offset(frame.start_address().as_u64());
        increase_ref(frame);

        if zero_page {
            unsafe {
                let start_ptr = (write_addr as usize) as *mut u8;
                core::ptr::write_bytes(start_ptr, 0, 4096);
            }
        }

        (frame, write_addr)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AppliedPageCluster {
    pub frame: PhysFrame,
    pub file_lazy_stats: FileLazyFaultStats,
}
