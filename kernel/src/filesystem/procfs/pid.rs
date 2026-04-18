use alloc::{format, string::String, vec, vec::Vec};

use crate::{
    filesystem::{
        errors::FSError, info::DirectoryContentInfo, vfs::FSResult,
        vfs_traits::DirectoryContentType,
    },
    process::{
        manager::{MANAGER, get_current_process},
        misc::{ProcessID, get_process_with_pid},
    },
};

pub(super) fn pid_dir_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("fd".into(), DirectoryContentType::Directory),
    ]
}

pub(super) fn pid_fd_entries(pid: ProcessID) -> FSResult<Vec<DirectoryContentInfo>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let mut entries = Vec::new();

    for (fd, object) in process.objects.iter().enumerate() {
        if object.is_some() {
            entries.push(DirectoryContentInfo::new(
                format!("{fd}"),
                DirectoryContentType::Symlink,
            ));
        }
    }

    Ok(entries)
}

pub(super) fn proc_pid_cmdline_bytes(_pid: ProcessID) -> Vec<u8> {
    Vec::new()
}

pub(super) fn parse_pid(pid: &str) -> FSResult<ProcessID> {
    pid.parse::<u64>()
        .map(ProcessID)
        .map_err(|_| FSError::NotFound)
}

pub(super) fn parse_fd(fd: &str) -> FSResult<&str> {
    fd.parse::<usize>().map_err(|_| FSError::NotFound)?;
    Ok(fd)
}

pub(super) fn ensure_pid_exists(pid: ProcessID) -> FSResult<()> {
    if MANAGER.lock().processes.contains_key(&pid) {
        Ok(())
    } else {
        Err(FSError::NotFound)
    }
}

pub(super) fn current_pid() -> FSResult<ProcessID> {
    Ok(get_current_process().lock().pid)
}

pub(super) fn pid_string(pid: ProcessID) -> String {
    format!("{}", pid.0)
}

pub(super) fn pid_dir_inode(pid: ProcessID) -> u64 {
    0x4000_0000 + pid.0 * 2
}

pub(super) fn pid_cmdline_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 1
}

pub(super) fn pid_fd_dir_inode(pid: ProcessID) -> u64 {
    0x5000_0000 + pid.0 * 0x100
}

pub(super) fn pid_fd_inode(pid: ProcessID, fd: &str) -> u64 {
    pid_fd_dir_inode(pid) + 1 + fd.parse::<u64>().unwrap_or(0)
}

pub(super) fn fd_target(pid: ProcessID, fd: &str) -> FSResult<String> {
    let fd_index = fd.parse::<usize>().map_err(|_| FSError::NotFound)?;
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let object = process
        .objects
        .get(fd_index)
        .and_then(|entry| entry.clone())
        .ok_or(FSError::NotFound)?;

    if let Ok(file_like) = object.clone().as_file_like() {
        if let Ok(info) = file_like.info() {
            if info.name.starts_with('/') {
                return Ok(info.name);
            }
            return Ok(format!("/{}", info.name));
        }
    }

    Ok(format!("anon_inode:[{}]", object.debug_name()))
}
