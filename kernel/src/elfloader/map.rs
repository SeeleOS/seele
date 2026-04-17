use alloc::sync::Arc;
use elfloader::ElfLoaderErr;
use xmas_elf::{ElfFile, program::Type};

use crate::{
    elfloader::{
        ElfInfo, headers::read_interp, load_base::choose_load_base_offset,
        segment::load_segment_to_area,
    },
    filesystem::object::FileLikeObject,
    memory::addrspace::AddrSpace,
    misc::time::with_profiling,
};

fn program_header_table_addr(elf: &ElfFile, load_base: u64) -> u64 {
    for header in elf.program_iter() {
        if matches!(header.get_type(), Ok(Type::Phdr)) {
            return load_base + header.virtual_addr();
        }
    }

    let ph_offset = elf.header.pt2.ph_offset();
    for header in elf.program_iter() {
        if !matches!(header.get_type(), Ok(Type::Load)) {
            continue;
        }

        let seg_start = header.offset();
        let seg_end = header.offset() + header.file_size();
        if ph_offset >= seg_start && ph_offset < seg_end {
            return load_base + header.virtual_addr() + (ph_offset - seg_start);
        }
    }

    load_base + ph_offset
}

pub fn load_elf_lazy(
    addrspace: &mut AddrSpace,
    file: Arc<FileLikeObject>,
    elf_bytes: &[u8],
) -> Result<ElfInfo, ElfLoaderErr> {
    let elf = ElfFile::new(elf_bytes)?;
    let load_base = choose_load_base_offset(addrspace, &elf);
    let mut interpreter = None;

    with_profiling(
        || {
            for header in elf.program_iter() {
                match header.get_type()? {
                    Type::Load => {
                        if header.mem_size() == 0 {
                            continue;
                        }

                        addrspace.register_area(load_segment_to_area(
                            header,
                            load_base,
                            file.clone(),
                        ));
                    }
                    Type::Interp => {
                        interpreter = Some(read_interp(&file, header).unwrap());
                    }
                    _ => {}
                }
            }
            Ok::<(), ElfLoaderErr>(())
        },
        "load_elf_lazy map segments",
    )?;

    Ok(ElfInfo {
        entry_point: load_base + elf.header.pt2.entry_point(),
        program_header_table: program_header_table_addr(&elf, load_base),
        program_header_count: elf.header.pt2.ph_count(),
        program_header_entry_size: elf.header.pt2.ph_entry_size(),
        interpreter,
        load_base,
    })
}
