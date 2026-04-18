use alloc::{format, vec, vec::Vec};

use crate::{
    filesystem::{info::DirectoryContentInfo, vfs_traits::DirectoryContentType},
    process::manager::MANAGER,
};

pub(super) const PROC_ROOT_INODE: u64 = 0x3000;
pub(super) const PROC_CMDLINE_INODE: u64 = 0x3001;
pub(super) const PROC_SELF_INODE: u64 = 0x3002;

pub(super) fn proc_root_entries() -> Vec<DirectoryContentInfo> {
    let mut entries = vec![
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
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
