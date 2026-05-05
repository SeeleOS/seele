use crate::{
    filesystem::{path::Path, vfs_traits::Whence},
    misc::error::AsSyscallError,
    object::{
        FileFlags,
        bpf::BpfObject,
        error::ObjectError,
        memfd::{memfd_add_seals, memfd_get_seals, register_memfd},
        open_state::OpenState,
    },
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
