use x86_64::structures::paging::PageTableFlags;
use xmas_elf::program::Flags;

use crate::elfloader::{
    ElfInfo,
    segment::elf_flags_to_page_flags,
    util::{align_down, align_up},
};
use crate::process::new::prefault_targets;

crate::test!(
    elf_alignment_helpers,
    "elf alignment helpers round to requested power of two",
    elf_alignment_helpers_round_to_requested_power_of_two
);
crate::test!(
    elf_segment_page_flags,
    "elf segment flags map to user page flags",
    elf_segment_flags_map_to_user_page_flags
);
crate::test!(
    elf_prefault_targets,
    "elf prefault targets include key entry points without duplicates",
    elf_prefault_targets_include_key_pages_without_duplicates
);

fn elf_alignment_helpers_round_to_requested_power_of_two() {
    assert_eq!(align_down(0x1234, 0x1000), 0x1000);
    assert_eq!(align_up(0x1234, 0x1000), 0x2000);
    assert_eq!(align_up(0x2000, 0x1000), 0x2000);
}

fn elf_segment_flags_map_to_user_page_flags() {
    let read_exec = elf_flags_to_page_flags(Flags(0b101));
    assert!(read_exec.contains(PageTableFlags::PRESENT));
    assert!(read_exec.contains(PageTableFlags::USER_ACCESSIBLE));
    assert!(!read_exec.contains(PageTableFlags::WRITABLE));
    assert!(!read_exec.contains(PageTableFlags::NO_EXECUTE));

    let read_write = elf_flags_to_page_flags(Flags(0b110));
    assert!(read_write.contains(PageTableFlags::WRITABLE));
    assert!(read_write.contains(PageTableFlags::NO_EXECUTE));
}

fn elf_prefault_targets_include_key_pages_without_duplicates() {
    let program = ElfInfo {
        entry_point: 0x401234,
        program_header_table: 0x400040,
        program_header_count: 3,
        program_header_entry_size: 56,
        interpreter: None,
        load_base: 0x400000,
        prefault_addrs: alloc::vec![0x401000, 0x402000, 0x401000],
    };
    let interpreter = ElfInfo {
        entry_point: 0x7f00_1234,
        program_header_table: 0x7f00_0040,
        program_header_count: 4,
        program_header_entry_size: 56,
        interpreter: None,
        load_base: 0x7f00_0000,
        prefault_addrs: alloc::vec![0x7f00_1000],
    };

    let targets = prefault_targets(&program, Some(&interpreter));

    assert!(targets.contains(&program.entry_point));
    assert!(targets.contains(&program.program_header_table));
    assert!(targets.contains(&interpreter.entry_point));
    assert!(targets.contains(&interpreter.program_header_table));
    assert!(targets.contains(&0x401000));
    assert_eq!(targets.len(), 7);
}
