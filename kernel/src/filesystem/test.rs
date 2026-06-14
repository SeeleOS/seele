use alloc::{format, string::String, sync::Arc, vec, vec::Vec};
use core::str;

use crate::filesystem::info::DirectoryContentInfo;
use crate::filesystem::vfs_traits::{DirectoryContentType, FileLike, FileSystem, Whence};
use crate::filesystem::{
    absolute_path::AbsolutePath,
    devfs::DevFs,
    errors::FSError,
    info::{FileLikeInfo, LinuxStat, UnixPermission},
    page_cache,
    path::{Path, PathPart},
    procfs::ProcFs,
    sparse_file::SparseFileData,
    staticfs::{
        StaticDirEntry, StaticDirectoryNode, StaticFileNode, StaticFs, StaticNode,
        StaticSymlinkNode,
    },
    sysfs::SysFs,
    tmpfs::TmpfsState,
    vfs::VirtualFS,
    vfs_operations::open_path,
    vfs_traits::{FileLikeType, MountFlags},
};
use crate::misc::utsname::{current_domainname, current_hostname, set_domainname, set_hostname};
use crate::object::tty_device::{get_active_vt, set_active_vt};
use crate::process::manager::get_current_process;

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
crate::test!(
    procfs_dynamic_entries_and_rw_nodes,
    "procfs exposes dynamic pid entries symlinks and writable control nodes",
    procfs_exposes_dynamic_pid_entries_symlinks_and_writable_control_nodes
);
crate::test!(
    sysfs_dynamic_nodes_and_uevent_payloads,
    "sysfs exposes dynamic tty state readonly nodes and stable uevent payloads",
    sysfs_exposes_dynamic_tty_state_readonly_nodes_and_stable_uevent_payloads
);
crate::test!(
    page_cache_reuses_cached_file_pages,
    "page cache reuses cached file pages for repeated reads",
    page_cache_reuses_cached_file_pages_for_repeated_reads
);
crate::test!(
    page_cache_second_chance_eviction,
    "page cache second-chance eviction preserves referenced pages",
    page_cache_second_chance_eviction_preserves_referenced_pages
);
crate::test!(
    page_cache_cluster_reuse,
    "page cache reuses cached cluster across neighboring pages",
    page_cache_reuses_cached_cluster_for_neighboring_pages
);
crate::test!(
    vfs_open_without_reborrowing_virtualfs,
    "vfs open returns opened files without reborrowing the global virtualfs refcell",
    vfs_open_returns_opened_files_without_reborrowing_virtualfs
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

fn ext4_test_path(name: &str) -> Path {
    Path::new(&format!("/{}", name))
}

fn page_cache_reuses_cached_file_pages_for_repeated_reads() {
    let path = ext4_test_path("page-cache-test-file");
    let _ = VirtualFS.lock().delete_file(path.clone());
    VirtualFS.lock().create_file(path.clone()).unwrap();

    let opened = Arc::new(open_path(path.clone()).unwrap());
    opened.write_exact_at(b"cache me", 0).unwrap();
    let (wrapped, identity) = opened.readonly_page_cache_file().unwrap();
    page_cache::invalidate_file(identity.file);

    let first = page_cache::read_page(&wrapped, identity, 0).unwrap();
    let second = page_cache::read_page(&wrapped, identity, 0).unwrap();
    assert!(!first.was_hit);
    assert!(second.was_hit);
    assert_eq!(&second.data[..8], b"cache me");

    let _ = VirtualFS.lock().delete_file(path);
}

fn vfs_open_returns_opened_files_without_reborrowing_virtualfs() {
    let path = Path::new("/tmp/vfs-open-no-reborrow");
    let _ = VirtualFS.lock().delete_file(path.clone());
    VirtualFS.lock().create_file(path.clone()).unwrap();

    let opened = VirtualFS
        .lock()
        .open(path.clone())
        .expect("opening through the locked VFS should not reborrow VirtualFS");
    opened.write_exact_at(b"ok", 0).unwrap();

    let reopened = open_path(path.clone()).unwrap();
    let mut buf = [0u8; 2];
    reopened.read_exact_at(&mut buf, 0).unwrap();
    assert_eq!(&buf, b"ok");

    let _ = VirtualFS.lock().delete_file(path);
}

fn page_cache_second_chance_eviction_preserves_referenced_pages() {
    let hot_file = page_cache::FileCacheKey {
        device_id: 1,
        inode: 1,
    };
    let cold_file = page_cache::FileCacheKey {
        device_id: 1,
        inode: 2,
    };
    let new_file = page_cache::FileCacheKey {
        device_id: 1,
        inode: 3,
    };

    page_cache::reset_for_test();
    page_cache::insert_for_test(hot_file, 0, b'h', true);
    for page_index in (16..(16 * 1024)).step_by(16) {
        page_cache::insert_for_test(cold_file, page_index, b'c', false);
    }

    assert!(page_cache::contains_for_test(hot_file, 0));
    assert!(page_cache::contains_for_test(cold_file, 16));

    page_cache::insert_for_test(new_file, 0, b'n', true);

    assert!(page_cache::contains_for_test(hot_file, 0));
    assert!(!page_cache::contains_for_test(cold_file, 16));
    assert!(page_cache::contains_for_test(new_file, 0));

    page_cache::reset_for_test();
}

fn page_cache_reuses_cached_cluster_for_neighboring_pages() {
    let path = ext4_test_path("page-cache-cluster-test-file");
    let _ = VirtualFS.lock().delete_file(path.clone());
    VirtualFS.lock().create_file(path.clone()).unwrap();

    let opened = open_path(path.clone()).unwrap();
    let contents = (0..(4096 * 2))
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    opened.write_exact_at(&contents, 0).unwrap();
    let (wrapped, identity) = opened.readonly_page_cache_file().unwrap();
    page_cache::invalidate_file(identity.file);

    let first = page_cache::read_page(&wrapped, identity, 0).unwrap();
    let second = page_cache::read_page(&wrapped, identity, 1).unwrap();
    assert!(!first.was_hit);
    assert!(second.was_hit);
    assert_eq!(&second.data[4096..4096 + 32], &contents[4096..4096 + 32]);

    let _ = VirtualFS.lock().delete_file(path);
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

fn procfs_exposes_dynamic_pid_entries_symlinks_and_writable_control_nodes() {
    let fs = ProcFs::new();
    let current_pid = get_current_process().lock().pid;
    let current_pid_name = format!("{}", current_pid.0);

    let FileLike::Directory(root) = fs.lookup(&Path::new("/")).unwrap() else {
        panic!("/proc root should be a directory");
    };
    let root_names = root
        .lock()
        .contents()
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert!(root_names.contains(&current_pid_name));
    assert!(root_names.contains(&"self".into()));

    let FileLike::Symlink(self_link) = fs.lookup(&Path::new("/self")).unwrap() else {
        panic!("/proc/self should be a symlink");
    };
    assert_eq!(
        self_link.lock().target().unwrap().as_string(),
        current_pid_name
    );

    let FileLike::Symlink(root_link) = fs.lookup(&Path::new("/self/root")).unwrap() else {
        panic!("/proc/self/root should be a symlink");
    };
    assert_eq!(root_link.lock().target().unwrap().as_string(), "/");

    let FileLike::Directory(pid_ns) = fs.lookup(&Path::new("/self/ns")).unwrap() else {
        panic!("/proc/self/ns should be a directory");
    };
    let pid_ns_info = pid_ns.lock().info().unwrap();
    assert_eq!(pid_ns_info.name, "ns");

    let FileLike::File(net_ns) = fs.lookup(&Path::new("/self/ns/net")).unwrap() else {
        panic!("/proc/self/ns/net should be a file-like namespace node");
    };
    let net_ns_inode = net_ns.lock().info().unwrap().inode;
    assert_ne!(net_ns_inode, 0);

    let FileLike::Directory(fd_dir) = fs.lookup(&Path::new("/self/fd")).unwrap() else {
        panic!("/proc/self/fd should be a directory");
    };
    let fd_entries = fd_dir
        .lock()
        .contents()
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    let expected_fds = {
        let process = get_current_process();
        let process = process.lock();
        process
            .fd_table
            .lock()
            .iter()
            .enumerate()
            .filter_map(|(fd, entry)| entry.as_ref().map(|_| format!("{fd}")))
            .collect::<Vec<_>>()
    };
    for fd in expected_fds {
        assert!(fd_entries.contains(&fd), "missing fd entry {fd}");
    }

    let hostname_before = c_string_field_to_string(current_hostname(crate::NAME));
    let domainname_before = c_string_field_to_string(current_domainname("(none)"));

    let FileLike::File(hostname) = fs.lookup(&Path::new("/sys/kernel/hostname")).unwrap() else {
        panic!("hostname should be a writable proc file");
    };
    let mut hostname = hostname.lock();
    assert_eq!(hostname.write(b" proc-host \n").unwrap(), 12);
    hostname.seek(0, Whence::Start).unwrap();
    let mut hostname_bytes = [0; 32];
    let hostname_read = hostname.read(&mut hostname_bytes).unwrap();
    assert_eq!(
        str::from_utf8(&hostname_bytes[..hostname_read]).unwrap(),
        "proc-host\n"
    );
    assert!(matches!(hostname.write(&[0xff]), Err(FSError::Other)));
    set_hostname(hostname_before.as_bytes()).unwrap();

    let FileLike::File(domainname) = fs.lookup(&Path::new("/sys/kernel/domainname")).unwrap()
    else {
        panic!("domainname should be a writable proc file");
    };
    let mut domainname = domainname.lock();
    assert_eq!(domainname.write(b" example.test \0").unwrap(), 15);
    domainname.seek(0, Whence::Start).unwrap();
    let mut domainname_bytes = [0; 32];
    let domainname_read = domainname.read(&mut domainname_bytes).unwrap();
    assert_eq!(
        str::from_utf8(&domainname_bytes[..domainname_read]).unwrap(),
        "example.test\n"
    );
    assert!(matches!(domainname.write(&[0xff]), Err(FSError::Other)));
    set_domainname(domainname_before.as_bytes()).unwrap();

    let FileLike::File(file_max) = fs.lookup(&Path::new("/sys/fs/file-max")).unwrap() else {
        panic!("file-max should be writable");
    };
    let mut file_max = file_max.lock();
    assert_eq!(file_max.write(b" 12345\n").unwrap(), 7);
    file_max.seek(0, Whence::Start).unwrap();
    let mut file_max_bytes = [0; 32];
    let file_max_read = file_max.read(&mut file_max_bytes).unwrap();
    assert_eq!(
        str::from_utf8(&file_max_bytes[..file_max_read]).unwrap(),
        "12345\n"
    );
    assert!(matches!(
        file_max.write(b"not-a-number"),
        Err(FSError::Other)
    ));

    let oom_path = format!("/{}/oom_score_adj", current_pid.0);
    let FileLike::File(oom_score_adj) = fs.lookup(&Path::new(&oom_path)).unwrap() else {
        panic!("oom_score_adj should be writable");
    };
    let mut oom_score_adj = oom_score_adj.lock();
    assert_eq!(oom_score_adj.write(b"1000\n").unwrap(), 5);
    oom_score_adj.seek(0, Whence::Start).unwrap();
    let mut oom_bytes = [0; 16];
    let oom_read = oom_score_adj.read(&mut oom_bytes).unwrap();
    assert_eq!(str::from_utf8(&oom_bytes[..oom_read]).unwrap(), "1000\n");
    assert!(matches!(oom_score_adj.write(b"1001"), Err(FSError::Other)));
    assert!(matches!(oom_score_adj.write(b"abc"), Err(FSError::Other)));
    oom_score_adj.write(b"0").unwrap();

    let FileLike::File(pressure_cpu) = fs.lookup(&Path::new("/pressure/cpu")).unwrap() else {
        panic!("pressure cpu node should be writable");
    };
    let mut pressure_cpu = pressure_cpu.lock();
    assert_eq!(
        pressure_cpu.write(b"some 150000 1000000").unwrap(),
        b"some 150000 1000000".len()
    );
    pressure_cpu.seek(0, Whence::Start).unwrap();
    let mut pressure_bytes = [0; 128];
    let pressure_read = pressure_cpu.read(&mut pressure_bytes).unwrap();
    let rendered = str::from_utf8(&pressure_bytes[..pressure_read]).unwrap();
    assert!(rendered.contains("some avg10=0.00"));
    assert!(rendered.contains("full avg10=0.00"));
}

fn sysfs_exposes_dynamic_tty_state_readonly_nodes_and_stable_uevent_payloads() {
    let fs = SysFs::new();

    let active_before = get_active_vt();
    let FileLike::File(active) = fs.lookup(&Path::new("/class/tty/tty0/active")).unwrap() else {
        panic!("tty0/active should be a file");
    };
    let mut active = active.lock();
    let mut active_bytes = [0; 16];
    let first_read = active.read(&mut active_bytes).unwrap();
    assert_eq!(
        str::from_utf8(&active_bytes[..first_read]).unwrap(),
        format!("tty{}\n", active_before)
    );
    assert!(matches!(active.write(b"tty2"), Err(FSError::Readonly)));

    assert!(set_active_vt(2));
    active.seek(0, Whence::Start).unwrap();
    let second_read = active.read(&mut active_bytes).unwrap();
    assert_eq!(
        str::from_utf8(&active_bytes[..second_read]).unwrap(),
        "tty2\n"
    );
    assert!(set_active_vt(active_before));

    for (path, expected_lines) in [
        ("/devices/uevent", vec!["SUBSYSTEM=devices"]),
        ("/devices/platform/uevent", vec!["SUBSYSTEM=platform"]),
        (
            "/devices/platform/i8042/uevent",
            vec![
                "DRIVER=i8042",
                "MODALIAS=platform:i8042",
                "SUBSYSTEM=platform",
            ],
        ),
    ] {
        let FileLike::File(node) = fs.lookup(&Path::new(path)).unwrap() else {
            panic!("{path} should be a file");
        };
        let mut node = node.lock();
        let mut bytes = [0; 128];
        let read = node.read(&mut bytes).unwrap();
        let rendered = str::from_utf8(&bytes[..read]).unwrap();
        for line in expected_lines {
            assert!(rendered.contains(line), "{path} missing {line}");
        }
    }

    let FileLike::Symlink(subsystem) = fs
        .lookup(&Path::new(
            "/devices/platform/i8042/serio0/input/input0/event0/subsystem",
        ))
        .unwrap()
    else {
        panic!("event0 subsystem should be a symlink");
    };
    assert_eq!(
        subsystem.lock().target().unwrap().as_string(),
        "/sys/class/input"
    );
}

fn c_string_field_to_string(field: [u8; 65]) -> String {
    let len = field
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(field.len());
    str::from_utf8(&field[..len]).unwrap().into()
}
