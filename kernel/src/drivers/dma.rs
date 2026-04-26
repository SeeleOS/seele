use alloc::{collections::BTreeMap, vec::Vec};
use core::ptr::NonNull;

use conquer_once::spin::OnceCell;
use spin::Mutex;
use x86_64::structures::paging::{PageSize, Size4KiB};

use crate::memory::{paging::FRAME_ALLOCATOR, utils::apply_offset};

const PAGE_SIZE: usize = Size4KiB::SIZE as usize;

type DmaPagePool = BTreeMap<usize, Vec<(u64, usize)>>;

static DMA_PAGE_POOL: OnceCell<Mutex<DmaPagePool>> = OnceCell::uninit();

fn dma_page_pool() -> &'static Mutex<DmaPagePool> {
    DMA_PAGE_POOL.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn allocate_dma_pages(pages: usize) -> Option<(u64, NonNull<u8>)> {
    if let Some((paddr, vaddr)) = dma_page_pool().lock().get_mut(&pages).and_then(Vec::pop) {
        unsafe {
            core::ptr::write_bytes(vaddr as *mut u8, 0, pages * PAGE_SIZE);
        }
        return Some((paddr, NonNull::new(vaddr as *mut u8)?));
    }

    let mut allocator = FRAME_ALLOCATOR.get().unwrap().lock();
    let start = allocator.allocate_contiguous(pages)?;
    let paddr = start.start_address().as_u64();
    let vaddr = apply_offset(paddr) as *mut u8;

    unsafe {
        core::ptr::write_bytes(vaddr, 0, pages * PAGE_SIZE);
    }

    Some((paddr, NonNull::new(vaddr)?))
}

pub fn deallocate_dma_pages(paddr: u64, pages: usize) {
    let vaddr = apply_offset(paddr) as usize;
    dma_page_pool()
        .lock()
        .entry(pages)
        .or_default()
        .push((paddr, vaddr));
}

#[derive(Debug)]
pub struct DmaRegion {
    paddr: u64,
    vaddr: NonNull<u8>,
    len: usize,
    pages: usize,
}

unsafe impl Send for DmaRegion {}
unsafe impl Sync for DmaRegion {}

impl DmaRegion {
    pub fn new(len: usize) -> Option<Self> {
        let pages = len.div_ceil(PAGE_SIZE).max(1);
        let (paddr, vaddr) = allocate_dma_pages(pages)?;
        Some(Self {
            paddr,
            vaddr,
            len,
            pages,
        })
    }

    pub fn phys_addr(&self) -> u64 {
        self.paddr
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr<T>(&self) -> *const T {
        self.vaddr.as_ptr().cast::<T>()
    }

    pub fn as_mut_ptr<T>(&self) -> *mut T {
        self.vaddr.as_ptr().cast::<T>()
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.vaddr.as_ptr(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.vaddr.as_ptr(), self.len) }
    }
}

impl Drop for DmaRegion {
    fn drop(&mut self) {
        deallocate_dma_pages(self.paddr, self.pages);
    }
}
