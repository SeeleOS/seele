use conquer_once::spin::OnceCell;
use core::mem::MaybeUninit;
use limine::{
    framebuffer::Framebuffer,
    memory_map::{Entry, EntryType},
    request::{FramebufferRequest, HhdmRequest, MemoryMapRequest, RsdpRequest, StackSizeRequest},
};

const KERNEL_STACK_SIZE: u64 = 2 * 1024 * 1024;

#[used]
#[unsafe(link_section = ".requests")]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(KERNEL_STACK_SIZE);

#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryRegionKind {
    Usable,
    Reserved,
}

#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub start: u64,
    pub end: u64,
    pub kind: MemoryRegionKind,
}

pub fn init() {}

pub fn physical_memory_offset() -> u64 {
    HHDM_REQUEST
        .get_response()
        .expect("limine hhdm response missing")
        .offset()
}

pub fn memory_map() -> &'static [MemoryRegion] {
    const MAX_MEMORY_REGIONS: usize = 256;
    static MEMORY_REGION_COUNT: OnceCell<usize> = OnceCell::uninit();
    static mut MEMORY_REGIONS: [MaybeUninit<MemoryRegion>; MAX_MEMORY_REGIONS] =
        [const { MaybeUninit::uninit() }; MAX_MEMORY_REGIONS];

    let len = *MEMORY_REGION_COUNT.get_or_init(|| {
        let entries = MEMORY_MAP_REQUEST
            .get_response()
            .expect("limine memory map response missing")
            .entries();
        assert!(
            entries.len() <= MAX_MEMORY_REGIONS,
            "limine memory map has too many entries"
        );
        for (index, entry) in entries.iter().enumerate() {
            unsafe {
                MEMORY_REGIONS[index].write(convert_memory_region(entry));
            }
        }
        entries.len()
    });

    unsafe {
        let ptr = (&raw const MEMORY_REGIONS).cast::<MemoryRegion>();
        core::slice::from_raw_parts(ptr, len)
    }
}

fn convert_memory_region(entry: &Entry) -> MemoryRegion {
    MemoryRegion {
        start: entry.base,
        end: entry.base.saturating_add(entry.length),
        kind: if entry.entry_type == EntryType::USABLE {
            MemoryRegionKind::Usable
        } else {
            MemoryRegionKind::Reserved
        },
    }
}

pub fn framebuffer() -> Framebuffer<'static> {
    FRAMEBUFFER_REQUEST
        .get_response()
        .expect("limine framebuffer response missing")
        .framebuffers()
        .next()
        .expect("limine framebuffer missing")
}

pub fn rsdp_address() -> u64 {
    RSDP_REQUEST
        .get_response()
        .expect("limine rsdp response missing")
        .address() as u64
}
