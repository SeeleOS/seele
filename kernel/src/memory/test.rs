use crate::filesystem::{
    page_cache,
    path::Path,
    vfs::{VirtualFS, WrappedFile},
    vfs_operations::open_path,
};
use crate::memory::{
    addrspace::AddrSpace,
    paging::{BootinfoFrameAllocator, align_down_4k, align_up_4k},
    protection::Protection,
};
use alloc::{vec, vec::Vec};

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
crate::test!(
    file_lazy_copy_whole_page,
    "file lazy copy fast path copies full cached page",
    file_lazy_copy_fast_path_copies_full_cached_page
);
crate::test!(
    file_lazy_copy_partial_tail,
    "file lazy copy preserves partial tail and zero fill",
    file_lazy_copy_preserves_partial_tail_and_zero_fill
);
crate::test!(
    file_lazy_copy_unaligned_offset,
    "file lazy copy handles unaligned offsets across pages",
    file_lazy_copy_handles_unaligned_offsets_across_pages
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

fn write_test_file(path: &Path, data: &[u8]) {
    let _ = VirtualFS.lock().delete_file(path.clone());
    VirtualFS.lock().create_file(path.clone()).unwrap();
    let opened = open_path(path.clone()).unwrap();
    opened.write_exact_at(data, 0).unwrap();
    let (_wrapped, identity) = opened.readonly_page_cache_file().unwrap();
    page_cache::invalidate_file(identity.file);
}

fn with_test_cached_file<T>(
    path_str: &str,
    data: &[u8],
    f: impl FnOnce(&WrappedFile, page_cache::FileCacheIdentity) -> T,
) -> T {
    let path = Path::new(path_str);
    write_test_file(&path, data);
    let opened = open_path(path.clone()).unwrap();
    let (wrapped, identity) = opened.readonly_page_cache_file().unwrap();
    let result = f(&wrapped, identity);
    let _ = VirtualFS.lock().delete_file(path);
    result
}

fn file_lazy_copy_fast_path_copies_full_cached_page() {
    let data = vec![0x5a; 4096];
    with_test_cached_file("/tmp/file-lazy-copy-page", &data, |wrapped, identity| {
        let (buffer, stats) =
            AddrSpace::copy_cached_file_page_for_test(wrapped, identity, 0, 4096, 4096).unwrap();
        assert_eq!(buffer, data);
        assert_eq!(stats.cluster_pages_loaded, 1);
        assert_eq!(stats.cache_hits + stats.cache_misses, 1);
    });
}

fn file_lazy_copy_preserves_partial_tail_and_zero_fill() {
    let data = b"tail-data".to_vec();
    with_test_cached_file("/tmp/file-lazy-copy-tail", &data, |wrapped, identity| {
        let (buffer, stats) =
            AddrSpace::copy_cached_file_page_for_test(wrapped, identity, 0, data.len(), 4096)
                .unwrap();
        assert_eq!(&buffer[..data.len()], data.as_slice());
        assert!(buffer[data.len()..].iter().all(|byte| *byte == 0));
        assert_eq!(stats.cluster_pages_loaded, 1);
    });
}

fn file_lazy_copy_handles_unaligned_offsets_across_pages() {
    let data = (0..5000)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    with_test_cached_file(
        "/tmp/file-lazy-copy-unaligned",
        &data,
        |wrapped, identity| {
            let offset = 4096 - 32;
            let read_len = 96usize;
            let (buffer, stats) = AddrSpace::copy_cached_file_page_for_test(
                wrapped,
                identity,
                offset as u64,
                read_len,
                128,
            )
            .unwrap();
            assert_eq!(&buffer[..read_len], &data[offset..offset + read_len]);
            assert!(buffer[read_len..].iter().all(|byte| *byte == 0));
            assert_eq!(stats.cluster_pages_loaded, 1);
            #[cfg(feature = "profiling")]
            assert!(stats.cache_lookup_cycles > 0);
            assert!(stats.cache_hits + stats.cache_misses >= 1);
        },
    );
}
