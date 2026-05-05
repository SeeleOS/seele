use alloc::vec;

use crate::filesystem::{
    absolute_path::AbsolutePath,
    errors::FSError,
    path::{Path, PathPart},
    sparse_file::SparseFileData,
    tmpfs::TmpfsState,
};

crate::test!("filesystem broad pure helpers", || {
    path_normalization_handles_absolute_and_relative_components();
    absolute_path_respects_root_jail();
    sparse_file_reads_holes_as_zeroes();
    tmpfs_state_tracks_children_and_empty_directory_rules();
});

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
