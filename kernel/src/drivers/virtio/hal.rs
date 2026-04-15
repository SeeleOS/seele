use alloc::{collections::BTreeMap, vec::Vec};
use conquer_once::spin::OnceCell;
use core::ptr::NonNull;
use spin::Mutex;
use virtio_drivers::{
    BufferDirection, Hal, PhysAddr,
};
use x86_64::{
    structures::paging::{PageSize, Size4KiB},
};

use crate::memory::{
    mmio::map_mmio,
    paging::FRAME_ALLOCATOR,
    utils::apply_offset,
};

const PAGE_SIZE: usize = Size4KiB::SIZE as usize;

#[derive(Debug)]
struct SharedAllocation {
    vaddr: NonNull<u8>,
    pages: usize,
    len: usize,
    direction: BufferDirection,
}

// SAFETY: The raw pointer points to uniquely-owned DMA memory.
unsafe impl Send for SharedAllocation {}

static SHARED_ALLOCATIONS: OnceCell<Mutex<BTreeMap<PhysAddr, SharedAllocation>>> =
    OnceCell::uninit();
static DMA_PAGE_POOL: OnceCell<Mutex<BTreeMap<usize, Vec<(PhysAddr, usize)>>>> =
    OnceCell::uninit();

fn shared_allocations() -> &'static Mutex<BTreeMap<PhysAddr, SharedAllocation>> {
    SHARED_ALLOCATIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn dma_page_pool() -> &'static Mutex<BTreeMap<usize, Vec<(PhysAddr, usize)>>> {
    DMA_PAGE_POOL.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn allocate_dma_pages(pages: usize) -> Option<(PhysAddr, NonNull<u8>)> {
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

fn deallocate_dma_pages(paddr: PhysAddr, pages: usize) {
    let vaddr = apply_offset(paddr) as usize;
    dma_page_pool()
        .lock()
        .entry(pages)
        .or_default()
        .push((paddr, vaddr));
}

pub struct KernelHal;

unsafe impl Hal for KernelHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        allocate_dma_pages(pages).unwrap_or((0, NonNull::dangling()))
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        deallocate_dma_pages(paddr, pages);
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(map_mmio(paddr, _size) as *mut u8).unwrap()
    }

    unsafe fn share(buffer: NonNull<[u8]>, direction: BufferDirection) -> PhysAddr {
        let slice = unsafe { buffer.as_ref() };
        let len = slice.len();
        let pages = len.div_ceil(PAGE_SIZE).max(1);
        let (paddr, vaddr) = allocate_dma_pages(pages).expect("virtio: failed to allocate DMA");

        if matches!(
            direction,
            BufferDirection::DriverToDevice | BufferDirection::Both
        ) {
            unsafe {
                core::ptr::copy_nonoverlapping(slice.as_ptr(), vaddr.as_ptr(), len);
            }
        }

        shared_allocations().lock().insert(
            paddr,
            SharedAllocation {
                vaddr,
                pages,
                len,
                direction,
            },
        );

        paddr
    }

    unsafe fn unshare(paddr: PhysAddr, buffer: NonNull<[u8]>, direction: BufferDirection) {
        let allocation = shared_allocations()
            .lock()
            .remove(&paddr)
            .expect("virtio: missing shared DMA allocation");

        debug_assert_eq!(allocation.direction, direction);

        if matches!(
            direction,
            BufferDirection::DeviceToDriver | BufferDirection::Both
        ) {
            let dst = buffer.as_ptr() as *mut u8;

            unsafe {
                core::ptr::copy_nonoverlapping(allocation.vaddr.as_ptr(), dst, allocation.len);
            }
        }

        deallocate_dma_pages(paddr, allocation.pages);
    }
}
