use alloc::{format, string::String, vec, vec::Vec};

use crate::{
    filesystem::{
        cgroupfs::pid_cgroup_path, errors::FSError, info::DirectoryContentInfo, vfs::FSResult,
        vfs_traits::DirectoryContentType,
    },
    process::{
        manager::{MANAGER, get_current_process},
        misc::{ProcessID, get_process_with_pid},
    },
};

pub(super) fn pid_dir_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("stat".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("cgroup".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("oom_score_adj".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("mountinfo".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("fd".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("fdinfo".into(), DirectoryContentType::Directory),
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

pub(super) fn proc_pid_stat_bytes(pid: ProcessID) -> FSResult<Vec<u8>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let parent_pid = process.parent.as_ref().map(|parent| parent.lock().pid.0).unwrap_or(0);
    let state = if process.have_exited() || process.threads.is_empty() {
        'Z'
    } else {
        'S'
    };
    let comm = pid_string(pid);
    let session = process.group_id.0;
    let num_threads = process.threads.len().max(1);
    let content = format!(
        concat!(
            "{} ({}) {} {} {} {} 0 0 0 0 0 0 0 0 0 0 0 0 20 0 {} 0 ",
            "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n"
        ),
        pid.0,
        comm,
        state,
        parent_pid,
        process.group_id.0,
        session,
        num_threads,
    );
    Ok(content.into_bytes())
}

pub(super) fn proc_pid_cmdline_bytes(_pid: ProcessID) -> Vec<u8> {
    Vec::new()
}

pub(super) fn proc_pid_cgroup_bytes(pid: ProcessID) -> Vec<u8> {
    format!("0::{}\n", pid_cgroup_path(pid)).into_bytes()
}

pub(super) fn proc_pid_oom_score_adj_bytes(pid: ProcessID) -> FSResult<Vec<u8>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    Ok(format!("{}\n", process.lock().oom_score_adj).into_bytes())
}

pub(super) fn proc_pid_write_oom_score_adj(pid: ProcessID, buffer: &[u8]) -> FSResult<usize> {
    let content = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
    let value = content
        .trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0')
        .parse::<i32>()
        .map_err(|_| FSError::Other)?;
    if !(-1000..=1000).contains(&value) {
        return Err(FSError::Other);
    }

    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    process.lock().oom_score_adj = value;
    Ok(buffer.len())
}

pub(super) fn pid_fdinfo_entries(pid: ProcessID) -> FSResult<Vec<DirectoryContentInfo>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let mut entries = Vec::new();

    for (fd, object) in process.objects.iter().enumerate() {
        if object.is_some() {
            entries.push(DirectoryContentInfo::new(
                format!("{fd}"),
                DirectoryContentType::File,
            ));
        }
    }

    Ok(entries)
}

pub(super) fn proc_pid_fdinfo_bytes(pid: ProcessID, fd: usize) -> FSResult<Vec<u8>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let object = process
        .objects
        .get(fd)
        .and_then(|entry| entry.clone())
        .ok_or(FSError::NotFound)?;

    let mut content = format!("pos:\t0\nflags:\t0\nmnt_id:\t0\nino:\t0\n");
    if let Ok(pidfd) = object.as_pidfd() {
        content.push_str(&format!("Pid:\t{}\n", pidfd.pid()));
    }

    Ok(content.into_bytes())
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

pub(super) fn pid_mountinfo_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 2
}

pub(super) fn pid_cgroup_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 3
}

pub(super) fn pid_oom_score_adj_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 4
}

pub(super) fn pid_stat_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 5
}

pub(super) fn pid_fd_dir_inode(pid: ProcessID) -> u64 {
    0x5000_0000 + pid.0 * 0x100
}

pub(super) fn pid_fdinfo_dir_inode(pid: ProcessID) -> u64 {
    0x5100_0000 + pid.0 * 0x100
}

pub(super) fn pid_fd_inode(pid: ProcessID, fd: &str) -> u64 {
    pid_fd_dir_inode(pid) + 1 + fd.parse::<u64>().unwrap_or(0)
}

pub(super) fn pid_fdinfo_inode(pid: ProcessID, fd: &str) -> u64 {
    pid_fdinfo_dir_inode(pid) + 1 + fd.parse::<u64>().unwrap_or(0)
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
        return Ok(file_like.path().as_string());
    }

    Ok(format!("anon_inode:[{}]", object.debug_name()))
}
