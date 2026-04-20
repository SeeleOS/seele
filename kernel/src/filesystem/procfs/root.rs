use alloc::{collections::BTreeMap, format, string::String, vec, vec::Vec};
use lazy_static::lazy_static;

use crate::{
    filesystem::{
        info::DirectoryContentInfo,
        vfs::{FileSystemRef, VirtualFS},
        vfs_traits::DirectoryContentType,
    },
    misc::time::Time,
    process::manager::MANAGER,
};

pub(super) const PROC_ROOT_INODE: u64 = 0x3000;
pub(super) const PROC_CMDLINE_INODE: u64 = 0x3001;
pub(super) const PROC_SELF_INODE: u64 = 0x3002;
pub(super) const PROC_MOUNTS_INODE: u64 = 0x3003;
pub(super) const PROC_SYS_INODE: u64 = 0x3004;
pub(super) const PROC_SYS_FS_INODE: u64 = 0x3005;
pub(super) const PROC_SYS_FS_FILE_MAX_INODE: u64 = 0x3006;
pub(super) const PROC_SYS_FS_NR_OPEN_INODE: u64 = 0x3007;
pub(super) const PROC_SYS_KERNEL_INODE: u64 = 0x3008;
pub(super) const PROC_SYS_KERNEL_RANDOM_INODE: u64 = 0x3009;
pub(super) const PROC_SYS_KERNEL_RANDOM_BOOT_ID_INODE: u64 = 0x300a;

lazy_static! {
    static ref PROC_BOOT_ID: String = generate_boot_id();
}

pub(super) fn proc_root_entries() -> Vec<DirectoryContentInfo> {
    let mut entries = vec![
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("mounts".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("self".into(), DirectoryContentType::Symlink),
        DirectoryContentInfo::new("sys".into(), DirectoryContentType::Directory),
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

pub(super) fn proc_kernel_entries() -> Vec<DirectoryContentInfo> {
    vec![DirectoryContentInfo::new(
        "random".into(),
        DirectoryContentType::Directory,
    )]
}

pub(super) fn proc_kernel_random_entries() -> Vec<DirectoryContentInfo> {
    vec![DirectoryContentInfo::new(
        "boot_id".into(),
        DirectoryContentType::File,
    )]
}

pub(super) fn proc_boot_id_bytes() -> Vec<u8> {
    format!("{}\n", PROC_BOOT_ID.as_str()).into_bytes()
}

fn generate_boot_id() -> String {
    let mut state = Time::current().as_nanoseconds()
        ^ Time::since_boot().as_nanoseconds().rotate_left(19)
        ^ 0x6a09_e667_f3bc_c908;
    let mut bytes = [0u8; 16];

    for byte in &mut bytes {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }

    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn sorted_mounts() -> Vec<(String, FileSystemRef)> {
    let mut mounts = VirtualFS
        .lock()
        .mount_snapshots()
        .into_iter()
        .map(|(path, fs)| (path.as_string(), fs))
        .collect::<Vec<_>>();
    mounts.sort_by_key(|(path, _)| (path.matches('/').count(), path.len()));
    mounts
}

pub(super) fn proc_mounts_bytes() -> Vec<u8> {
    let mut out = String::new();
    for (path, fs) in sorted_mounts() {
        let fs = fs.lock();
        out.push_str(fs.mount_source());
        out.push(' ');
        out.push_str(&path);
        out.push(' ');
        out.push_str(fs.name());
        out.push(' ');
        out.push_str(fs.mount_options(&crate::filesystem::path::Path::new(&path)));
        out.push_str(" 0 0\n");
    }
    out.into_bytes()
}

pub(super) fn proc_mountinfo_bytes() -> Vec<u8> {
    let mounts = sorted_mounts();
    let mut ids = BTreeMap::new();
    for (index, (path, _)) in mounts.iter().enumerate() {
        ids.insert(path.clone(), index as u64 + 1);
    }

    let mut out = String::new();
    for (path, fs) in mounts {
        let id = *ids.get(&path).unwrap_or(&1);
        let parent_id = if path == "/" {
            0
        } else {
            ids
                .keys()
                .filter(|candidate| {
                    candidate.as_str() != path
                        && (path == format!("{}/", candidate.trim_end_matches('/'))
                            || path.starts_with(&format!("{}/", candidate.trim_end_matches('/'))))
                })
                .max_by_key(|candidate| candidate.len())
                .and_then(|candidate| ids.get(candidate))
                .copied()
                .unwrap_or(1)
        };
        let fs = fs.lock();
        let options = fs.mount_options(&crate::filesystem::path::Path::new(&path));
        out.push_str(&format!(
            "{id} {parent_id} 0:{id} / {path} {options} - {} {} rw\n",
            fs.name(),
            fs.mount_source()
        ));
    }
    out.into_bytes()
}
