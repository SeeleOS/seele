use x86_64::structures::paging::PageTableFlags;
use xmas_elf::program::Flags;

use crate::elfloader::{
    segment::elf_flags_to_page_flags,
    util::{align_down, align_up},
};

crate::test!("elfloader alignment and flag helpers", || {
    elf_alignment_helpers_round_to_requested_power_of_two();
    elf_segment_flags_map_to_user_page_flags();
});

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
