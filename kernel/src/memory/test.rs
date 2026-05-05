use crate::memory::{
    paging::{BootinfoFrameAllocator, align_down_4k, align_up_4k},
    protection::Protection,
};

crate::test!(
    paging_page_count_normalization,
    "paging helpers normalize page counts",
    paging_helpers_normalize_page_counts
);
crate::test!(
    four_k_alignment_bounds,
    "4k alignment helpers round bounds",
    four_k_alignment_helpers_round_bounds
);
crate::test!(
    protection_flags_bitfield,
    "protection flags are closed syscall bitfield",
    protection_flags_are_closed_syscall_bitfield
);

fn paging_helpers_normalize_page_counts() {
    assert_eq!(BootinfoFrameAllocator::normalized_pages(0), None);
    assert_eq!(BootinfoFrameAllocator::normalized_pages(1), Some(1));
    assert_eq!(BootinfoFrameAllocator::normalized_pages(3), Some(4));
    assert_eq!(BootinfoFrameAllocator::normalized_pages(8), Some(8));
}

fn four_k_alignment_helpers_round_bounds() {
    assert_eq!(align_down_4k(0), 0);
    assert_eq!(align_down_4k(0x1fff), 0x1000);
    assert_eq!(align_up_4k(0x1001), 0x2000);
    assert_eq!(align_up_4k(0x2000), 0x2000);
}

fn protection_flags_are_closed_syscall_bitfield() {
    let rw = Protection::READ | Protection::WRITE;

    assert!(rw.contains(Protection::READ));
    assert!(rw.contains(Protection::WRITE));
    assert!(!rw.contains(Protection::EXEC));
    assert_eq!(
        Protection::from_bits(rw.bits()).map(|flags| flags.bits()),
        Some(rw.bits())
    );
    assert!(Protection::from_bits(1 << 9).is_none());
}
