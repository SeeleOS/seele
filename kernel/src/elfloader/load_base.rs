use x86_64::VirtAddr;
use xmas_elf::{
    ElfFile,
    header::{self},
    program::Type,
};

use crate::elfloader::util::{align_down, align_up};
use crate::memory::addrspace::AddrSpace;

fn load_span(elf: &ElfFile) -> Option<(u64, u64)> {
    let mut min_addr: Option<u64> = None;
    let mut max_addr = 0;

    for header in elf.program_iter() {
        if !matches!(header.get_type(), Ok(Type::Load)) || header.mem_size() == 0 {
            continue;
        }

        let start = align_down(header.virtual_addr(), 4096);
        let end = align_up(header.virtual_addr() + header.mem_size(), 4096);

        min_addr = Some(min_addr.map_or(start, |current| current.min(start)));
        max_addr = max_addr.max(end);
    }

    min_addr.map(|min_addr| (min_addr, max_addr))
}

pub fn choose_load_base_offset(addrspace: &mut AddrSpace, elf: &ElfFile) -> u64 {
    if elf.header.pt2.type_().as_type() != header::Type::SharedObject {
        return 0;
    }

    let Some((min_addr, max_addr)) = load_span(elf) else {
        return 0;
    };

    let region_start = align_up(addrspace.user_mem.as_u64(), 4096);
    let span = max_addr - min_addr;
    addrspace.user_mem = VirtAddr::new(region_start + span + 4096);
    region_start - min_addr
}
