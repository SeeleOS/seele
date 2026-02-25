use core::{cmp::min, ptr::copy_nonoverlapping};

use elfloader::ElfBinary;
use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, OffsetPageTable, PageTableFlags,
        Translate,
    },
};

use crate::memory::{
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
    page_table: &'a mut OffsetPageTable<'static>,
}

impl<'a> ElfLoader<'a> {
    pub fn new(page_table: &'a mut OffsetPageTable<'static>) -> Self {
        Self { page_table }
    }
}

impl<'a> elfloader::ElfLoader for ElfLoader<'a> {
    fn allocate(
        &mut self,
        load_headers: elfloader::LoadableHeaders,
    ) -> Result<(), elfloader::ElfLoaderErr> {
        for header in load_headers {
            // TODO: use the proper flags
            let page_range = page_range_from_size(header.virtual_addr(), header.mem_size());
            let flags = PageTableFlags::USER_ACCESSIBLE
                | PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE;

            for page in page_range {
                if let Ok(_frame) = self.page_table.translate_page(page) {
                    unsafe {
                        self.page_table.update_flags(page, flags).unwrap().flush();
                    }
                    continue;
                }

                let frame = FRAME_ALLOCATOR
                    .get()
                    .unwrap()
                    .lock()
                    .allocate_frame()
                    .unwrap();

                unsafe {
                    self.page_table
                        .map_to(
                            page,
                            frame,
                            flags,
                            &mut *FRAME_ALLOCATOR.get().unwrap().lock(),
                        )
                        .unwrap()
                        .flush();
                };
            }
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

        while offset < region.len() {
            let addr = addr + offset as u64;
            let phys_addr = self.page_table.translate_addr(addr).unwrap();
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
pub fn load_elf<'a>(page_table: &mut PageTableWrapped, program: &'a [u8]) -> ElfBinary<'a> {
    let binary = ElfBinary::new(program).expect("Failed to parse elf binary");

    binary
        .load(&mut ElfLoader::new(&mut page_table.inner))
        .expect("Failed to load ELF");

    binary
}
