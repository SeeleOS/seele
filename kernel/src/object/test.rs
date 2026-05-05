use crate::{
    filesystem::{path::Path, vfs_traits::Whence},
    misc::error::AsSyscallError,
    object::{
        FileFlags,
        bpf::BpfObject,
        error::ObjectError,
        file_locks::{
            AdvisoryLock, AdvisoryLockApi, AdvisoryLockOwner, AdvisoryLockRange, AdvisoryLockType,
            F_RDLCK, F_WRLCK, apply_posix_lock, find_conflict, parse_flock_operation,
            ranges_overlap,
        },
        memfd::{memfd_add_seals, memfd_get_seals, register_memfd},
        open_state::OpenState,
    },
    process::misc::ProcessID,
    systemcall::utils::SyscallError,
};

crate::test!(
    open_state_file_flags,
    "open state tracks file flags",
    open_state_tracks_file_flags
);
crate::test!(
    object_error_syscall_mapping,
    "object errors map to syscall errors",
    object_errors_map_to_syscall_errors
);
crate::test!(
    bpf_array_default_entries,
    "bpf array maps default missing entries to zero",
    bpf_array_maps_default_missing_entries_to_zero
);
crate::test!(
    memfd_seal_rules,
    "memfd registry applies seal rules",
    memfd_registry_applies_seal_rules
);
crate::test!(
    file_lock_range_conflicts,
    "file lock ranges overlap and detect read write conflicts",
    file_lock_ranges_overlap_and_detect_read_write_conflicts
);
crate::test!(
    file_lock_posix_merge,
    "posix file locks merge adjacent ranges and split unlocks",
    posix_file_locks_merge_adjacent_ranges_and_split_unlocks
);
crate::test!(
    flock_operation_parsing,
    "flock operation parser accepts one mode plus nonblock",
    flock_operation_parser_accepts_one_mode_plus_nonblock
);

fn open_state_tracks_file_flags() {
    let state = OpenState::new(FileFlags::NONBLOCK);

    assert!(state.contains(FileFlags::NONBLOCK));
    state.set_flags(FileFlags::APPEND);
    assert_eq!(state.get_flags(), FileFlags::APPEND);
}

fn object_errors_map_to_syscall_errors() {
    assert_eq!(
        ObjectError::DoesNotExist.as_syscall_error(),
        SyscallError::BadFileDescriptor
    );
    assert_eq!(
        ObjectError::InvalidRequest.as_syscall_error(),
        SyscallError::InappropriateIoctl
    );
}

fn bpf_array_maps_default_missing_entries_to_zero() {
    let map = BpfObject::new_map(2, 4, 3, 2);

    assert_eq!(
        map.lookup_map_element(&0u32.to_ne_bytes()).unwrap(),
        [0, 0, 0]
    );
    map.update_map_element(&1u32.to_ne_bytes(), &[7, 8, 9])
        .unwrap();
    assert_eq!(
        map.lookup_map_element(&1u32.to_ne_bytes()).unwrap(),
        [7, 8, 9]
    );
    assert!(matches!(
        map.lookup_map_element(&2u32.to_ne_bytes()),
        Err(SyscallError::FileNotFound)
    ));
}

fn memfd_registry_applies_seal_rules() {
    const F_SEAL_SEAL: u32 = 0x0001;
    const F_SEAL_WRITE: u32 = 0x0008;

    let path = Path::new("/memfd:test");
    register_memfd(&path, true);
    assert_eq!(memfd_get_seals(&path), Some(0));
    assert_eq!(memfd_add_seals(&path, F_SEAL_WRITE), Ok(0));
    assert_eq!(memfd_get_seals(&path), Some(F_SEAL_WRITE));
    assert_eq!(memfd_add_seals(&path, F_SEAL_SEAL), Ok(0));
    assert!(matches!(
        memfd_add_seals(&path, F_SEAL_WRITE),
        Err(SyscallError::PermissionDenied)
    ));

    let no_sealing = Path::new("/memfd:no-sealing");
    register_memfd(&no_sealing, false);
    assert_eq!(memfd_get_seals(&no_sealing), Some(F_SEAL_SEAL));
    assert!(matches!(
        memfd_add_seals(&no_sealing, F_SEAL_WRITE),
        Err(SyscallError::PermissionDenied)
    ));

    assert!(matches!(
        memfd_add_seals(&Path::new("/memfd:missing"), F_SEAL_WRITE),
        Err(SyscallError::InvalidArguments)
    ));
    assert!(matches!(Whence::try_from(0), Ok(Whence::Start)));
}

fn file_lock_ranges_overlap_and_detect_read_write_conflicts() {
    let owner_a = AdvisoryLockOwner::Process(ProcessID(1));
    let owner_b = AdvisoryLockOwner::Process(ProcessID(2));
    let read_lock = AdvisoryLock {
        api: AdvisoryLockApi::Posix,
        owner: owner_a,
        lock_type: AdvisoryLockType::Read,
        range: AdvisoryLockRange {
            start: 10,
            end: Some(20),
        },
    };
    let write_lock = AdvisoryLock {
        api: AdvisoryLockApi::Posix,
        owner: owner_b,
        lock_type: AdvisoryLockType::Write,
        range: AdvisoryLockRange {
            start: 15,
            end: Some(25),
        },
    };

    assert!(ranges_overlap(read_lock.range, write_lock.range));
    assert!(!ranges_overlap(
        read_lock.range,
        AdvisoryLockRange {
            start: 20,
            end: Some(30)
        }
    ));
    assert_eq!(
        find_conflict(
            &[read_lock],
            owner_b,
            Some(write_lock),
            AdvisoryLockApi::Posix
        )
        .map(|lock| lock.owner),
        Some(owner_a)
    );
    assert_eq!(
        find_conflict(
            &[read_lock],
            owner_b,
            Some(AdvisoryLock {
                lock_type: AdvisoryLockType::Read,
                ..write_lock
            }),
            AdvisoryLockApi::Posix
        )
        .map(|lock| lock.owner),
        None
    );
}

fn posix_file_locks_merge_adjacent_ranges_and_split_unlocks() {
    let owner = AdvisoryLockOwner::Process(ProcessID(7));
    let mut entries = Vec::new();

    apply_posix_lock(
        &mut entries,
        owner,
        crate::object::file_locks::ParsedFlockRequest {
            lock_type: Some(AdvisoryLockType::Write),
            range: AdvisoryLockRange {
                start: 0,
                end: Some(10),
            },
        },
    );
    apply_posix_lock(
        &mut entries,
        owner,
        crate::object::file_locks::ParsedFlockRequest {
            lock_type: Some(AdvisoryLockType::Write),
            range: AdvisoryLockRange {
                start: 10,
                end: Some(20),
            },
        },
    );

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].range.start, 0);
    assert_eq!(entries[0].range.end, Some(20));

    apply_posix_lock(
        &mut entries,
        owner,
        crate::object::file_locks::ParsedFlockRequest {
            lock_type: None,
            range: AdvisoryLockRange {
                start: 5,
                end: Some(15),
            },
        },
    );

    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].range,
        AdvisoryLockRange {
            start: 0,
            end: Some(5)
        }
    );
    assert_eq!(
        entries[1].range,
        AdvisoryLockRange {
            start: 15,
            end: Some(20)
        }
    );
}

fn flock_operation_parser_accepts_one_mode_plus_nonblock() {
    assert_eq!(
        parse_flock_operation(1).unwrap(),
        Some(AdvisoryLockType::Read)
    );
    assert_eq!(
        parse_flock_operation(2 | 4).unwrap(),
        Some(AdvisoryLockType::Write)
    );
    assert_eq!(parse_flock_operation(8).unwrap(), None);
    assert!(matches!(
        parse_flock_operation(1 | 2),
        Err(SyscallError::InvalidArguments)
    ));
    assert!(matches!(
        parse_flock_operation(1 | 0x100),
        Err(SyscallError::InvalidArguments)
    ));
    assert_eq!(F_RDLCK, 0);
    assert_eq!(F_WRLCK, 1);
}
