use core::{cmp::min, ptr::copy_nonoverlapping};

use elfloader::ElfBinary;
use x86_64::{
    VirtAddr,
    structures::paging::{FrameAllocator, Mapper, OffsetPageTable, PageTableFlags, Translate},
};

use crate::memory::{
    addrspace::AddrSpace,
    page_table_wrapper::PageTableWrapped,
    paging::FRAME_ALLOCATOR,
    utils::{apply_offset, page_range_from_size},
};

pub type Function = *const extern "C" fn() -> !;

// ELF is a file format that contains the actual code and instructions on which parts of the
// code need to be loaded where, and which parts of the file are instructions,
// which parts are memory, and which parts of the memory are read-only.
#[derive(Debug)]
pub struct ElfLoader<'a> {
    addrspace: &'a mut AddrSpace,
}

impl<'a> ElfLoader<'a> {
    pub fn new(page_table: &'a mut AddrSpace) -> Self {
        Self {
            addrspace: page_table,
        }
    }
}

impl<'a> elfloader::ElfLoader for ElfLoader<'a> {
    fn allocate(
        &mut self,
        load_headers: elfloader::LoadableHeaders,
    ) -> Result<(), elfloader::ElfLoaderErr> {
        for header in load_headers {
            // TODO: use the proper flags
            let flags = PageTableFlags::USER_ACCESSIBLE
                | PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE;
            let pages = header.mem_size().div_ceil(4096);

            log::debug!(
                "elf alloc: vaddr {:#x} mem {} bytes pages {}",
                header.virtual_addr(),
                header.mem_size(),
                pages
            );
            self.addrspace
                .map_no_guard_page(VirtAddr::new(header.virtual_addr()), pages, flags);
        }
        Ok(())
    }

    fn load(
        &mut self,
        _flags: elfloader::Flags,
        base: elfloader::VAddr,
        region: &[u8],
    ) -> Result<(), elfloader::ElfLoaderErr> {
        let addr = VirtAddr::new(base);
        let mut offset = 0;
        log::trace!("elf load base {:#x}", base);

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

/// Returns the entry point
pub fn load_elf<'a>(addrspace: &mut AddrSpace, program: &'a [u8]) -> ElfBinary<'a> {
    log::debug!("load_elf: start ({} bytes)", program.len());
    let binary = ElfBinary::new(program).expect("Failed to parse elf binary");

    binary
        .load(&mut ElfLoader::new(addrspace))
        .expect("Failed to load ELF");

    log::debug!("load_elf: done");
    binary
}
