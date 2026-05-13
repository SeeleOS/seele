use core::cell::{RefCell, RefMut};

use x86_64::{
    VirtAddr,
    structures::paging::{Page, PageTable, PageTableFlags, page::PageRangeInclusive},
};

use crate::memory::{PHYSICAL_MEMORY_OFFSET, addrspace::cow::COW_FLAG, paging::MAPPER};

pub type MutGuard<'a, T> = RefMut<'a, T>;

#[derive(Debug)]
pub struct Mut<T: ?Sized> {
    inner: RefCell<T>,
}

impl<T> Mut<T> {
    pub const fn new(inner: T) -> Self {
        Self {
            inner: RefCell::new(inner),
        }
    }
}

impl<T: Default> Default for Mut<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: ?Sized> Mut<T> {
    #[track_caller]
    pub fn lock(&self) -> MutGuard<'_, T> {
        self.inner.borrow_mut()
    }
}

// Debug-only lock replacement: we intentionally allow global/shared access so
// RefCell borrow panics expose re-entrant lock paths. This is not a real lock.
unsafe impl<T: ?Sized + Send> Send for Mut<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mut<T> {}

pub type Locked<A> = Mut<A>;

/// Copies the memory mapping of the kernel l4 table
/// into the table of something else (probably table for processes)
pub fn copy_kernel_mapping(table: &mut PageTable) {
    let l4_binding = MAPPER.get().unwrap().lock();
    let kernel_l4 = l4_binding.level_4_table();
    //s_println!("{:#?}", kernel_l4[0]);

    for i in 128..512 {
        table[i] = kernel_l4[i].clone();
    }
}

pub fn apply_offset(num: u64) -> u64 {
    num + PHYSICAL_MEMORY_OFFSET.get().unwrap()
}

pub fn page_range_from_size(start: u64, size: u64) -> PageRangeInclusive {
    page_range_from_addr(start, start + size - 1u64)
}

pub fn page_range_from_addr(start: u64, end: u64) -> PageRangeInclusive {
    let heap_start = VirtAddr::new(start);
    let heap_end = VirtAddr::new(end);
    let heap_start_page = Page::containing_address(heap_start);
    let heap_end_page = Page::containing_address(heap_end);
    Page::range_inclusive(heap_start_page, heap_end_page)
}

pub fn as_cow_flags(flags: PageTableFlags) -> PageTableFlags {
    (flags | COW_FLAG) & !PageTableFlags::WRITABLE
}
