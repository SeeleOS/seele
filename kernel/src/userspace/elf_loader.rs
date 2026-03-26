use core::{cmp::min, ptr::copy_nonoverlapping};

use elfloader::{BasicElf, DynamicElf, ElfBinary, LoadedElf};
use x86_64::{VirtAddr, structures::paging::PageTableFlags};

use crate::{
    memory::{
        addrspace::{
            AddrSpace,
            mem_area::{Data, MemoryArea},
        },
        utils::{apply_offset, page_range_from_size},
    },
};

pub type Function = *const extern "C" fn() -> !;

// ELF is a file format that contains the actual code and instructions on which parts of the
// code need to be loaded where, and which parts of the file are instructions,
// which parts are memory, and which parts of the memory are read-only.
#[derive(Debug)]
pub struct ElfLoader<'a> {
    addrspace: &'a mut AddrSpace,
    base_offset: u64,
}

impl<'a> ElfLoader<'a> {
    pub fn new(page_table: &'a mut AddrSpace, base_offset: u64) -> Self {
        Self {
            addrspace: page_table,
            base_offset,
        }
    }
}

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

fn load_span(binary: &ElfBinary) -> Option<(u64, u64)> {
    let mut min_addr: Option<u64> = None;
    let mut max_addr = 0;

    for header in binary.program_headers() {
        if header.mem_size() == 0 {
            continue;
        }

        let start = align_down(header.virtual_addr(), 4096);
        let end = align_up(header.virtual_addr() + header.mem_size(), 4096);

        min_addr = Some(min_addr.map_or(start, |current: u64| current.min(start)));
        max_addr = max_addr.max(end);
    }

    min_addr.map(|min_addr| (min_addr, max_addr))
}

fn choose_load_base_offset(addrspace: &mut AddrSpace, binary: &ElfBinary) -> u64 {
    if binary.program_header_table() != binary.file.header.pt2.ph_offset() {
        return 0;
    }

    let Some((min_addr, max_addr)) = load_span(binary) else {
        return 0;
    };

    let region_start = align_up(addrspace.user_mem.as_u64(), 4096);
    let span = max_addr - min_addr;
    addrspace.user_mem = VirtAddr::new(region_start + span + 4096);
    region_start - min_addr
}

fn apply_base_offset<'a>(loaded: LoadedElf<'a>, base_offset: u64) -> LoadedElf<'a> {
    match loaded {
        LoadedElf::Basic(info) => LoadedElf::Basic(BasicElf {
            entry_point: info.entry_point + base_offset,
            program_header_table: info.program_header_table + base_offset,
            ..info
        }),
        LoadedElf::Dynamic(info) => LoadedElf::Dynamic(DynamicElf {
            entry_point: info.entry_point + base_offset,
            program_header_table: info.program_header_table + base_offset,
            ..info
        }),
    }
}

impl<'a> elfloader::ElfLoader for ElfLoader<'a> {
    fn allocate(
        &mut self,
        load_headers: elfloader::LoadableHeaders,
    ) -> Result<(), elfloader::ElfLoaderErr> {
        for header in load_headers {
            let mem_size = header.mem_size();
            if mem_size == 0 {
                continue;
            }

            let mut flags = PageTableFlags::USER_ACCESSIBLE | PageTableFlags::PRESENT;
            if header.flags().is_write() {
                flags |= PageTableFlags::WRITABLE;
            }
            if !header.flags().is_execute() {
                flags |= PageTableFlags::NO_EXECUTE;
            }
            let start = align_down(self.base_offset + header.virtual_addr(), 4096);
            let end = align_up(self.base_offset + header.virtual_addr() + mem_size, 4096);
            let pages = (end - start) / 4096;

            log::debug!(
                "elf alloc: vaddr {:#x} mem {} bytes pages {}",
                start,
                mem_size,
                pages
            );
            self.addrspace.map(MemoryArea::new(
                VirtAddr::new(start),
                pages,
                flags,
                Data::Normal,
                false,
            ));
        }
        Ok(())
    }

    fn load(
        &mut self,
        _flags: elfloader::Flags,
        base: elfloader::VAddr,
        region: &[u8],
    ) -> Result<(), elfloader::ElfLoaderErr> {
        let addr = VirtAddr::new(self.base_offset + base);
        let mut offset = 0;
        log::trace!("elf load base {:#x}", self.base_offset + base);

        while offset < region.len() {
            let addr = addr + offset as u64;
            let phys_addr = self.addrspace.translate_addr(addr).unwrap();
            let phys_addr = apply_offset(phys_addr.as_u64());

            // Get how long the page lasts (We dont want to accidently write to
            // a different page, which might not be connected on the physical memory)
            let page_offset = phys_addr & 0xfff;
            let write_len = min(region.len() - offset, (4096 - page_offset) as usize);

            unsafe {
                copy_nonoverlapping(
                    // TODO is this correct?
                    region[offset..offset + write_len].as_ptr(),
                    phys_addr as *mut u8,
                    write_len,
                );
            }

            offset += write_len;
        }
        Ok(())
    }

    fn relocate(
        &mut self,
        _entry: elfloader::RelocationEntry,
    ) -> Result<(), elfloader::ElfLoaderErr> {
        Ok(())
    }
}

pub fn load_elf<'a>(addrspace: &mut AddrSpace, binary: &'a ElfBinary<'a>) -> LoadedElf<'a> {
    let base_offset = choose_load_base_offset(addrspace, binary);
    let loaded = binary
        .load(&mut ElfLoader::new(addrspace, base_offset))
        .unwrap();
    apply_base_offset(loaded, base_offset)
}
