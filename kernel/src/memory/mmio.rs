use alloc::collections::BTreeMap;
use conquer_once::spin::OnceCell;
use spin::Mutex;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{Mapper, Page, PageSize, PageTableFlags, PhysFrame, Size4KiB},
};

use crate::memory::{
    paging::{FRAME_ALLOCATOR, MAPPER},
    utils::page_range_from_addr,
};

const MMIO_BASE: u64 = 0xffff_fe00_0000_0000;

#[derive(Clone, Copy, Debug)]
struct MmioMapping {
    virt_base: u64,
}

static MMIO_STATE: OnceCell<Mutex<MmioState>> = OnceCell::uninit();

#[derive(Debug)]
struct MmioState {
    next_virt: u64,
    mappings: BTreeMap<(u64, usize), MmioMapping>,
}

impl MmioState {
    fn new() -> Self {
        Self {
            next_virt: MMIO_BASE,
            mappings: BTreeMap::new(),
        }
    }
}

fn state() -> &'static Mutex<MmioState> {
    MMIO_STATE.get_or_init(|| Mutex::new(MmioState::new()))
}

pub fn map_mmio(phys_addr: u64, size: usize) -> u64 {
    let page_mask = Size4KiB::SIZE - 1;
    let aligned_phys = phys_addr & !page_mask;
    let offset = phys_addr - aligned_phys;
    let aligned_size =
        ((offset as usize + size).div_ceil(Size4KiB::SIZE as usize)) * Size4KiB::SIZE as usize;

    let mut state = state().lock();
    if let Some(mapping) = state.mappings.get(&(aligned_phys, aligned_size)) {
        return mapping.virt_base + offset;
    }

    let virt_base = state.next_virt;
    state.next_virt += aligned_size as u64;

    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH
        | PageTableFlags::NO_EXECUTE;

    let page_range = page_range_from_addr(virt_base, virt_base + aligned_size as u64 - 1);
    let mut mapper = MAPPER.get().unwrap().lock();
    let mut frame_allocator = FRAME_ALLOCATOR.get().unwrap().lock();

    for (index, page) in page_range.enumerate() {
        let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(
            aligned_phys + index as u64 * Size4KiB::SIZE,
        ));

        unsafe {
            mapper
                .map_to(page, frame, flags, &mut *frame_allocator)
                .unwrap()
                .flush();
        }
    }

    state
        .mappings
        .insert((aligned_phys, aligned_size), MmioMapping { virt_base });

    log::debug!(
        "mmio: mapped phys={:#x} size={:#x} -> virt={:#x}",
        aligned_phys,
        aligned_size,
        virt_base,
    );

    virt_base + offset
}
