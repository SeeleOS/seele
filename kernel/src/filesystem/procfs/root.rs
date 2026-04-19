use alloc::{format, vec, vec::Vec};

use crate::{
    filesystem::{info::DirectoryContentInfo, vfs_traits::DirectoryContentType},
    process::manager::MANAGER,
};

pub(super) const PROC_ROOT_INODE: u64 = 0x3000;
pub(super) const PROC_CMDLINE_INODE: u64 = 0x3001;
pub(super) const PROC_SELF_INODE: u64 = 0x3002;
pub(super) const PROC_MOUNTS_INODE: u64 = 0x3003;

pub(super) fn proc_root_entries() -> Vec<DirectoryContentInfo> {
    let mut entries = vec![
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("mounts".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("self".into(), DirectoryContentType::Symlink),
    ];

    for pid in MANAGER.lock().processes.keys() {
        entries.push(DirectoryContentInfo::new(
            format!("{}", pid.0),
            DirectoryContentType::Directory,
        ));
    }

    entries
}

pub(super) fn proc_kernel_cmdline_bytes() -> Vec<u8> {
    Vec::new()
}

pub(super) fn proc_mounts_bytes() -> Vec<u8> {
    b"rootfs / ext4 rw,relatime 0 0\nproc /proc proc rw,nosuid,nodev,noexec,relatime 0 0\nsysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0\ncgroup2 /sys/fs/cgroup cgroup2 rw,nosuid,nodev,noexec,relatime 0 0\ndevtmpfs /dev devtmpfs rw,nosuid,relatime 0 0\n".to_vec()
}

pub(super) fn proc_mountinfo_bytes() -> Vec<u8> {
    b"1 0 0:1 / / rw,relatime - ext4 rootfs rw\n2 1 0:2 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw\n3 1 0:3 / /sys rw,nosuid,nodev,noexec,relatime - sysfs sysfs rw\n4 3 0:4 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime - cgroup2 cgroup2 rw\n5 1 0:5 / /dev rw,nosuid,relatime - devtmpfs devtmpfs rw\n".to_vec()
}
