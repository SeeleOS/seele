use alloc::sync::Arc;
use x86_64::{VirtAddr, structures::paging::PageTableFlags};
use xmas_elf::program::{Flags, ProgramHeader};

use crate::{
    elfloader::util::{align_down, align_up},
    filesystem::object::FileLikeObject,
    memory::{
        addrspace::mem_area::{Data, MemoryArea},
        protection::Protection,
    },
};

pub(crate) fn elf_flags_to_page_flags(flags: Flags) -> PageTableFlags {
    let mut page_flags = PageTableFlags::USER_ACCESSIBLE | PageTableFlags::PRESENT;
    if flags.is_write() {
        page_flags |= PageTableFlags::WRITABLE;
    }
    if !flags.is_execute() {
        page_flags |= PageTableFlags::NO_EXECUTE;
    }
    page_flags
}

pub(crate) fn elf_flags_to_protection(flags: Flags) -> Protection {
    let mut protection = Protection::READ;
    if flags.is_write() {
        protection |= Protection::WRITE;
    }
    if flags.is_execute() {
        protection |= Protection::EXEC;
    }
    protection
}

/// Loads the [`ProgramHeader`] into a [`MemoryArea`]
pub fn load_segment_to_area(
    header: ProgramHeader<'_>,
    load_base: u64,
    file: Arc<FileLikeObject>,
) -> MemoryArea {
    let page_delta = header.virtual_addr() & 0xfff;
    let start = align_down(load_base + header.virtual_addr(), 4096);
    let end = align_up(load_base + header.virtual_addr() + header.mem_size(), 4096);
    let file_offset = align_down(header.offset(), 4096);
    let file_bytes = header.file_size() + page_delta;

    MemoryArea {
        start: VirtAddr::new(start),
        end: VirtAddr::new(end),
        flags: elf_flags_to_page_flags(header.flags()),
        protection: elf_flags_to_protection(header.flags()),
        data: Data::File {
            offset: file_offset,
            file_bytes,
            file,
            shared: false,
        },
        lazy: true,
    }
}
