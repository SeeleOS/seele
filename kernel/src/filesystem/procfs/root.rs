use alloc::{collections::BTreeMap, format, string::String, vec, vec::Vec};

use crate::{
    filesystem::{
        info::DirectoryContentInfo,
        vfs::{FileSystemRef, VirtualFS},
        vfs_traits::DirectoryContentType,
    },
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
            let parent = ids
                .keys()
                .filter(|candidate| {
                    candidate.as_str() != path
                        && (path == format!("{}/", candidate.trim_end_matches('/'))
                            || path.starts_with(&format!("{}/", candidate.trim_end_matches('/'))))
                })
                .max_by_key(|candidate| candidate.len())
                .and_then(|candidate| ids.get(candidate))
                .copied()
                .unwrap_or(1);
            parent
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
