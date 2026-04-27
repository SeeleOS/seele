use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use spin::Mutex;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB, Translate,
};

use crate::{
    filesystem::object::mount_device_id_for_path,
    memory::{
        addrspace::{
            AddrSpace, AllocResult,
            cow::increase_ref,
            mem_area::{Data, MemoryArea},
        },
        paging::FRAME_ALLOCATOR,
        utils::apply_offset,
    },
    misc::stack_builder::StackBuilder,
};
use lazy_static::lazy_static;

const FILE_LAZY_CLUSTER_PAGES: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FilePageCacheKey {
    device_id: u64,
    inode: u64,
    offset: u64,
}

lazy_static! {
    static ref SHARED_FILE_PAGE_CACHE: Mutex<BTreeMap<FilePageCacheKey, PhysFrame>> =
        Mutex::new(BTreeMap::new());
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
                .inner
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

    fn readonly_file_page_cache_key(
        file: &Arc<crate::filesystem::object::FileLikeObject>,
        offset: u64,
    ) -> Option<FilePageCacheKey> {
        let info = file.info().ok()?;
        Some(FilePageCacheKey {
            device_id: mount_device_id_for_path(&file.path()),
            inode: info.inode,
            offset,
        })
    }

    fn get_or_load_shared_file_frame(
        file: &Arc<crate::filesystem::object::FileLikeObject>,
        offset: u64,
        read_len: usize,
    ) -> Option<PhysFrame> {
        let key = Self::readonly_file_page_cache_key(file, offset)?;
        let mut cache = SHARED_FILE_PAGE_CACHE.lock();
        if let Some(frame) = cache.get(&key).copied() {
            return Some(frame);
        }

        let frame = Self::alloc_zeroed_frame();
        if read_len != 0 {
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    apply_offset(frame.start_address().as_u64()) as *mut u8,
                    read_len,
                )
            };
            file.read_exact_at(dst, offset)
                .expect("Failed to lazyload readonly file-backed page");
        }

        increase_ref(frame);
        cache.insert(key, frame);
        Some(frame)
    }

    pub fn apply_page(&mut self, page: Page<Size4KiB>, area: MemoryArea) -> PhysFrame {
        self.apply_page_cluster(page, area, 1)
    }

    pub fn apply_page_cluster(
        &mut self,
        page: Page<Size4KiB>,
        area: MemoryArea,
        cluster_pages: u64,
    ) -> PhysFrame {
        match area.data {
            Data::Normal => self.alloc_map_zeroed_page(page, area, true).0,
            Data::File {
                offset,
                file_bytes,
                ref file,
                shared,
            } => unsafe {
                let use_shared_cache = shared || !area.flags.contains(PageTableFlags::WRITABLE);
                let page_index = (page.start_address().as_u64() - area.start.as_u64()) / 4096;
                let max_pages =
                    core::cmp::min(cluster_pages, area.pages().saturating_sub(page_index));
                let mut first_frame = None;

                for i in 0..max_pages {
                    let current_page = page + i;
                    if self
                        .page_table
                        .inner
                        .translate_addr(current_page.start_address())
                        .is_some()
                    {
                        continue;
                    }

                    let page_offset = current_page.start_address().as_u64() - area.start.as_u64();
                    let read_len = core::cmp::min(4096, file_bytes.saturating_sub(page_offset));
                    let file_offset = offset + page_offset;

                    if let (true, Some(frame)) = (
                        use_shared_cache,
                        Self::get_or_load_shared_file_frame(file, file_offset, read_len as usize),
                    ) {
                        let frame = self.map_existing_frame(current_page, frame, area.flags, true);
                        if first_frame.is_none() {
                            first_frame = Some(frame);
                        }
                        continue;
                    }

                    let (frame, write_addr) =
                        self.alloc_map_zeroed_page(current_page, area.clone(), read_len < 4096);

                    if first_frame.is_none() {
                        first_frame = Some(frame);
                    }

                    let read_len = read_len as usize;
                    if read_len != 0 {
                        let dst = core::slice::from_raw_parts_mut(write_addr as *mut u8, read_len);
                        file.read_exact_at(dst, file_offset)
                            .expect("Failed to lazyload file-backed page");
                    }
                }

                first_frame.unwrap_or_else(|| {
                    self.page_table
                        .inner
                        .translate_page(page)
                        .expect("file-backed cluster fault target page still unmapped")
                })
            },
            Data::Shared {
                ref frames,
                flags: shared_flags,
            } => {
                let page_index = (page.start_address().as_u64() - area.start.as_u64()) / 4096;
                let frame = frames[page_index as usize];
                self.map_existing_frame(page, frame, area.flags | shared_flags, false)
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
                    let frame = self.apply_page_cluster(page, area.clone(), 1);
                    page_write_bases.push(apply_offset(frame.start_address().as_u64()));
                }
            }
            Data::Normal => {
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
                .inner
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
