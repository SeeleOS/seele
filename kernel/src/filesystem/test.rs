use alloc::vec;

use crate::filesystem::{
    absolute_path::AbsolutePath,
    errors::FSError,
    info::{FileLikeInfo, LinuxStat, UnixPermission},
    path::{Path, PathPart},
    sparse_file::SparseFileData,
    tmpfs::TmpfsState,
    vfs_traits::{FileLikeType, MountFlags},
};

crate::test!(
    path_normalization_cases,
    "path normalization handles absolute and relative components",
    path_normalization_handles_absolute_and_relative_components
);
crate::test!(
    absolute_path_root_jail,
    "absolute path respects root jail",
    absolute_path_respects_root_jail
);
crate::test!(
    sparse_file_holes,
    "sparse file reads holes as zeroes",
    sparse_file_reads_holes_as_zeroes
);
crate::test!(
    tmpfs_child_state,
    "tmpfs state tracks children and empty directory rules",
    tmpfs_state_tracks_children_and_empty_directory_rules
);
crate::test!(
    linux_stat_mode_bits,
    "linux stat preserves explicit mode type bits and fills metadata",
    linux_stat_preserves_explicit_mode_type_bits_and_fills_metadata
);
crate::test!(
    mount_flag_proc_options,
    "mount flags render stable proc option strings",
    mount_flags_render_stable_proc_option_strings
);

fn path_normalization_handles_absolute_and_relative_components() {
    let normalized = Path::new("/usr//bin/../lib/./").normalize();

    assert_eq!(normalized.clone().as_string(), "/usr/lib/");
    assert!(normalized.is_absolute());
    assert!(normalized.ends_with_slash());
    assert_eq!(normalized.file_name().as_deref(), Some("lib"));
    assert_eq!(
        Path::new("../a/../../b").normalize().parts,
        vec![
            PathPart::ParentDir,
            PathPart::ParentDir,
            PathPart::Normal("b".into())
        ]
    );
    assert!(Path::new("/usr/lib64").starts_with(&Path::new("/usr")));
    assert!(
        Path::new("/usr/lib64")
            .strip_prefix(&Path::new("/opt"))
            .is_none()
    );
}

fn absolute_path_respects_root_jail() {
    let root = AbsolutePath::from_root_path(&Path::new("/sandbox"));
    let current = AbsolutePath::from_root_path(&Path::new("/sandbox/home/user"));

    let escaped = Path::new("../../../../etc/passwd").as_absolute_from(&root, &current);
    assert_eq!(escaped.display_string(&root), "/etc/passwd");

    let absolute_inside_root = Path::new("/var/log").as_absolute_from(&root, &current);
    assert_eq!(absolute_inside_root.as_string(), "/sandbox/var/log");
}

fn sparse_file_reads_holes_as_zeroes() {
    let mut data = SparseFileData::new();
    assert_eq!(data.write_at(4094, b"abcd"), 4);

    let mut buffer = [0xff; 8];
    assert_eq!(data.read_at(&mut buffer, 4092), 6);
    assert_eq!(&buffer[..6], &[0, 0, b'a', b'b', b'c', b'd']);

    data.truncate(4095);
    let mut tail = [0xff; 4];
    assert_eq!(data.read_at(&mut tail, 4094), 1);
    assert_eq!(tail[0], b'a');
}

fn tmpfs_state_tracks_children_and_empty_directory_rules() {
    let mut state = TmpfsState::new();

    state.create_directory("/", "run", 0o7777).unwrap();
    state.create_file("/run", "pid").unwrap();
    assert!(matches!(
        state.create_file("/run", "pid"),
        Err(FSError::AlreadyExists)
    ));
    assert!(matches!(
        state.delete_node("/", "run"),
        Err(FSError::DirectoryNotEmpty)
    ));

    state.delete_node("/run", "pid").unwrap();
    state.delete_node("/", "run").unwrap();
    assert!(matches!(state.node("/run"), Err(FSError::NotFound)));
}

fn linux_stat_preserves_explicit_mode_type_bits_and_fills_metadata() {
    let regular = FileLikeInfo::new("log".into(), 513, UnixPermission(0o640), FileLikeType::File)
        .with_owner(1000, 100)
        .with_inode(42)
        .as_linux();

    assert_eq!(regular.st_ino, 42);
    assert_eq!(regular.st_uid, 1000);
    assert_eq!(regular.st_gid, 100);
    assert_eq!(regular.st_mode, 0o100640);
    assert_eq!(regular.st_size, 513);
    assert_eq!(regular.st_blocks, 2);
    assert_eq!(regular.st_blksize, 4096);

    let symlink = FileLikeInfo::new(
        "link".into(),
        0,
        UnixPermission(0o120777),
        FileLikeType::File,
    )
    .as_linux();
    assert_eq!(symlink.st_mode, 0o120777);

    let char_device = LinuxStat::char_device_with_rdev(0o600, 0x1234);
    assert_eq!(char_device.st_mode, 0o020600);
    assert_eq!(char_device.st_rdev, 0x1234);
}

fn mount_flags_render_stable_proc_option_strings() {
    assert_eq!(MountFlags::empty().proc_options(), "rw");
    assert_eq!(
        (MountFlags::MS_RDONLY
            | MountFlags::MS_NOSUID
            | MountFlags::MS_NODEV
            | MountFlags::MS_NOEXEC
            | MountFlags::MS_RELATIME)
            .proc_options(),
        "ro,nosuid,nodev,noexec,relatime"
    );
}
