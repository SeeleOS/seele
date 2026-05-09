use alloc::{vec, vec::Vec};
use core::str;

use crate::filesystem::info::DirectoryContentInfo;
use crate::filesystem::vfs_traits::{DirectoryContentType, FileLike, FileSystem, Whence};
use crate::filesystem::{
    absolute_path::AbsolutePath,
    devfs::DevFs,
    errors::FSError,
    info::{FileLikeInfo, LinuxStat, UnixPermission},
    path::{Path, PathPart},
    procfs::ProcFs,
    sparse_file::SparseFileData,
    staticfs::{
        StaticDirEntry, StaticDirectoryNode, StaticFileNode, StaticFs, StaticNode,
        StaticSymlinkNode,
    },
    sysfs::SysFs,
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
crate::test!(
    staticfs_tree_rules,
    "staticfs exposes metadata tree shape and readonly rules",
    staticfs_exposes_metadata_tree_shape_and_readonly_rules
);
crate::test!(
    sysfs_static_tree_rules,
    "sysfs exposes stable metadata flags and static tree entries",
    sysfs_exposes_stable_metadata_flags_and_static_tree_entries
);
crate::test!(
    devfs_static_overlay_rules,
    "devfs preserves static overlay roots and readonly gating",
    devfs_preserves_static_overlay_roots_and_readonly_gating
);
crate::test!(
    procfs_static_entries,
    "procfs exposes stable static directories files and mount flags",
    procfs_exposes_stable_static_directories_files_and_mount_flags
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

fn static_test_file_bytes() -> Vec<u8> {
    b"hello".to_vec()
}

fn staticfs_exposes_metadata_tree_shape_and_readonly_rules() {
    static CONFIG_FILE: StaticNode = StaticNode::File(StaticFileNode {
        name: "config",
        inode: 0x41,
        mode: 0o100644,
        read: static_test_file_bytes,
        write: None,
    });
    static CONFIG_LINK: StaticNode = StaticNode::Symlink(StaticSymlinkNode {
        name: "latest",
        inode: 0x42,
        mode: 0o120777,
        target: "/etc/config",
    });
    static ETC_ENTRIES: &[StaticDirEntry] = &[StaticDirEntry {
        name: "config",
        node: &CONFIG_FILE,
    }];
    static ETC_DIR: StaticNode = StaticNode::Directory(StaticDirectoryNode {
        name: "etc",
        inode: 0x40,
        mode: 0o040755,
        entries: ETC_ENTRIES,
    });
    static ROOT_ENTRIES: &[StaticDirEntry] = &[
        StaticDirEntry {
            name: "etc",
            node: &ETC_DIR,
        },
        StaticDirEntry {
            name: "latest",
            node: &CONFIG_LINK,
        },
    ];
    static ROOT: StaticNode = StaticNode::Directory(StaticDirectoryNode {
        name: "/",
        inode: 0x3f,
        mode: 0o040755,
        entries: ROOT_ENTRIES,
    });

    let fs = StaticFs::new(&ROOT);
    assert_eq!(fs.name(), "staticfs");
    assert_eq!(fs.magic(), 0);
    assert_eq!(fs.mount_source(), "staticfs");
    assert_eq!(
        fs.default_mount_flags(&Path::new("/")),
        MountFlags::MS_RELATIME
    );
    assert!(matches!(
        fs.rename(&Path::new("/etc"), &Path::new("/tmp")),
        Err(FSError::Readonly)
    ));
    assert!(matches!(
        fs.link(&Path::new("/etc/config"), &Path::new("/etc/config.bak")),
        Err(FSError::Readonly)
    ));
    assert!(matches!(
        fs.lookup(&Path::new("../etc")),
        Err(FSError::NotADirectory)
    ));

    let FileLike::Directory(root) = fs.lookup(&Path::new("/")).unwrap() else {
        panic!("root should be a directory");
    };
    let root_names = root
        .lock()
        .contents()
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert_eq!(root_names, vec!["etc", "latest"]);

    let FileLike::Directory(etc) = fs.lookup(&Path::new("/etc")).unwrap() else {
        panic!("/etc should be a directory");
    };
    let etc_info = etc.lock().info().unwrap();
    assert_eq!(etc_info.name, "etc");
    assert_eq!(etc_info.inode, 0x40);
    assert_eq!(etc_info.permission.0, 0o040755);
    assert!(matches!(
        etc.lock().create(DirectoryContentInfo::new(
            "config".into(),
            DirectoryContentType::File
        )),
        Err(FSError::AlreadyExists)
    ));
    assert!(matches!(
        etc.lock().create(DirectoryContentInfo::new(
            "fresh".into(),
            DirectoryContentType::File
        )),
        Err(FSError::Readonly)
    ));

    let FileLike::File(file) = fs.lookup(&Path::new("/etc/config")).unwrap() else {
        panic!("config should be a file");
    };
    let mut file = file.lock();
    let mut buffer = [0; 8];
    assert_eq!(file.read(&mut buffer).unwrap(), 5);
    assert_eq!(&buffer[..5], b"hello");
    assert_eq!(file.seek(-1, Whence::End).unwrap(), 4);
    assert_eq!(file.read(&mut buffer[..2]).unwrap(), 1);
    assert_eq!(buffer[0], b'o');

    let FileLike::Symlink(link) = fs.lookup(&Path::new("/latest")).unwrap() else {
        panic!("latest should be a symlink");
    };
    assert_eq!(link.lock().target().unwrap().as_string(), "/etc/config");
}

fn sysfs_exposes_stable_metadata_flags_and_static_tree_entries() {
    let fs = SysFs::new();
    assert_eq!(fs.name(), "sysfs");
    assert_eq!(fs.magic(), 0x6265_6572);
    assert_eq!(fs.mount_source(), "sysfs");
    assert_eq!(
        fs.default_mount_flags(&Path::new("/")),
        MountFlags::MS_NOSUID
            | MountFlags::MS_NODEV
            | MountFlags::MS_NOEXEC
            | MountFlags::MS_RELATIME
    );
    assert!(matches!(
        fs.rename(&Path::new("/class"), &Path::new("/class2")),
        Err(FSError::Readonly)
    ));
    assert!(matches!(
        fs.link(&Path::new("/class"), &Path::new("/class2")),
        Err(FSError::Readonly)
    ));

    let FileLike::Directory(class_dir) = fs.lookup(&Path::new("/class")).unwrap() else {
        panic!("/class should be a directory");
    };
    let class_names = class_dir
        .lock()
        .contents()
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert_eq!(class_names, vec!["drm", "graphics", "input", "misc", "tty"]);

    let FileLike::File(uevent) = fs
        .lookup(&Path::new("/devices/platform/i8042/uevent"))
        .unwrap()
    else {
        panic!("i8042 uevent should be a file");
    };
    let uevent_info = uevent.lock().info().unwrap();
    assert_eq!(uevent_info.name, "uevent");
    assert_eq!(uevent_info.inode, 0x2064);
    assert_eq!(uevent_info.permission.0, 0o100644);

    let FileLike::Symlink(subsystem) = fs
        .lookup(&Path::new("/class/graphics/fb0/device/subsystem"))
        .unwrap()
    else {
        panic!("subsystem should be a symlink");
    };
    assert_eq!(
        subsystem.lock().target().unwrap().as_string(),
        "/sys/bus/platform"
    );
}

fn devfs_preserves_static_overlay_roots_and_readonly_gating() {
    let fs = DevFs::new();
    assert_eq!(fs.name(), "devtmpfs");
    assert_eq!(fs.magic(), 0x0102_1994);
    assert_eq!(fs.mount_source(), "devtmpfs");
    assert_eq!(
        fs.default_mount_flags(&Path::new("/")),
        MountFlags::MS_NOSUID | MountFlags::MS_RELATIME
    );

    let FileLike::Directory(root) = fs.lookup(&Path::new("/")).unwrap() else {
        panic!("/dev root should be a directory");
    };
    let root_entries = root.lock().contents().unwrap();
    assert_eq!(
        root_entries
            .iter()
            .filter(|entry| entry.name == "pts")
            .count(),
        1
    );
    assert!(root_entries.iter().any(|entry| entry.name == "input"));
    assert!(root_entries.iter().any(|entry| entry.name == "dri"));
    assert!(root_entries.iter().any(|entry| entry.name == "shm"));

    let FileLike::Directory(pts) = fs.lookup(&Path::new("/pts")).unwrap() else {
        panic!("/pts should be a directory");
    };
    let pts_info = pts.lock().info().unwrap();
    assert_eq!(pts_info.name, "pts");
    assert_eq!(pts_info.inode, 0x100c);
    assert_eq!(pts_info.permission.0, 0o040755);

    let FileLike::Symlink(log) = fs.lookup(&Path::new("/log")).unwrap() else {
        panic!("/log should be a symlink");
    };
    assert_eq!(
        log.lock().target().unwrap().as_string(),
        "/run/systemd/journal/dev-log"
    );

    assert!(matches!(
        fs.rename(&Path::new("/pts"), &Path::new("/pts2")),
        Err(FSError::Readonly)
    ));
    assert!(matches!(
        fs.rename(&Path::new("/shadow"), &Path::new("/null")),
        Err(FSError::Readonly)
    ));
    assert!(matches!(
        fs.link(&Path::new("/null"), &Path::new("/shadow")),
        Err(FSError::Readonly)
    ));
    assert!(matches!(
        fs.link(&Path::new("/shadow"), &Path::new("/ptmx")),
        Err(FSError::Readonly)
    ));
}

fn procfs_exposes_stable_static_directories_files_and_mount_flags() {
    let fs = ProcFs::new();
    assert_eq!(fs.name(), "proc");
    assert_eq!(fs.magic(), 0x9fa0);
    assert_eq!(fs.mount_source(), "proc");
    assert_eq!(
        fs.default_mount_flags(&Path::new("/")),
        MountFlags::MS_NOSUID
            | MountFlags::MS_NODEV
            | MountFlags::MS_NOEXEC
            | MountFlags::MS_RELATIME
    );
    assert!(matches!(
        fs.rename(&Path::new("/sys"), &Path::new("/sys2")),
        Err(FSError::Readonly)
    ));
    assert!(matches!(
        fs.link(&Path::new("/sys"), &Path::new("/sys2")),
        Err(FSError::Readonly)
    ));

    let FileLike::Directory(pressure) = fs.lookup(&Path::new("/pressure")).unwrap() else {
        panic!("/proc/pressure should be a directory");
    };
    let pressure_names = pressure
        .lock()
        .contents()
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert_eq!(pressure_names, vec!["cpu", "io", "memory"]);

    let FileLike::Directory(sys_dir) = fs.lookup(&Path::new("/sys")).unwrap() else {
        panic!("/proc/sys should be a directory");
    };
    let sys_names = sys_dir
        .lock()
        .contents()
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert_eq!(sys_names, vec!["fs", "kernel"]);

    let FileLike::File(osrelease) = fs.lookup(&Path::new("/sys/kernel/osrelease")).unwrap() else {
        panic!("osrelease should be a file");
    };
    let mut file = osrelease.lock();
    let mut bytes = [0; 32];
    let read = file.read(&mut bytes).unwrap();
    assert_eq!(str::from_utf8(&bytes[..read]).unwrap(), "6.12.0-seele\n");
}
