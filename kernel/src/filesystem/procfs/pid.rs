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
        DirectoryContentInfo::new("comm".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("stat".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("status".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("environ".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("cgroup".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("oom_score_adj".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("mountinfo".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("uid_map".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("gid_map".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("setgroups".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("root".into(), DirectoryContentType::Symlink),
        DirectoryContentInfo::new("ns".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("fd".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("fdinfo".into(), DirectoryContentType::Directory),
    ]
}

const PROC_NAMESPACE_NAMES: [&str; 8] =
    ["cgroup", "ipc", "mnt", "net", "pid", "time", "user", "uts"];

const PROC_CGROUP_INIT_INO: u64 = 0xEFFF_FFFB;
const PROC_IPC_INIT_INO: u64 = 0xEFFF_FFFF;
const PROC_MNT_INIT_INO: u64 = 0xEFFF_FFF8;
const PROC_NET_INIT_INO: u64 = 0xEFFF_FFF9;
const PROC_PID_INIT_INO: u64 = 0xEFFF_FFFC;
const PROC_TIME_INIT_INO: u64 = 0xEFFF_FFFA;
const PROC_USER_INIT_INO: u64 = 0xEFFF_FFFD;
const PROC_UTS_INIT_INO: u64 = 0xEFFF_FFFE;

fn default_user_namespace_map(id: u32) -> String {
    format!("0 {id} 1\n")
}

fn normalize_proc_control_write(buffer: &[u8]) -> FSResult<String> {
    let content = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
    Ok(if content.ends_with('\n') {
        String::from(content)
    } else {
        format!("{content}\n")
    })
}

pub(super) fn pid_ns_entries() -> Vec<DirectoryContentInfo> {
    PROC_NAMESPACE_NAMES
        .iter()
        .map(|name| DirectoryContentInfo::new((*name).into(), DirectoryContentType::File))
        .collect()
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
    let parent_pid = process
        .parent
        .as_ref()
        .map(|parent| parent.lock().pid.0)
        .unwrap_or(0);
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
        pid.0, comm, state, parent_pid, process.group_id.0, session, num_threads,
    );
    Ok(content.into_bytes())
}

pub(super) fn proc_pid_cmdline_bytes(pid: ProcessID) -> Vec<u8> {
    let Ok(process) = get_process_with_pid(pid) else {
        return Vec::new();
    };
    let process = process.lock();
    let mut bytes = Vec::new();
    for arg in &process.command_line {
        bytes.extend_from_slice(arg.as_bytes());
        bytes.push(0);
    }
    bytes
}

pub(super) fn proc_pid_comm_bytes(pid: ProcessID) -> FSResult<Vec<u8>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let name = process
        .command_line
        .first()
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .map(String::from)
        .unwrap_or_else(|| pid_string(pid));
    Ok(format!("{name}\n").into_bytes())
}

pub(super) fn proc_pid_environ_bytes(pid: ProcessID) -> FSResult<Vec<u8>> {
    let _ = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    Ok(Vec::new())
}

fn format_capability_set(low: u32, high: u32) -> String {
    format!("{:08x}{:08x}", high, low)
}

pub(super) fn proc_pid_status_bytes(pid: ProcessID) -> FSResult<Vec<u8>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    let parent_pid = process
        .parent
        .as_ref()
        .map(|parent| parent.lock().pid.0)
        .unwrap_or(0);
    let state = if process.have_exited() || process.threads.is_empty() {
        "Z (zombie)"
    } else {
        "S (sleeping)"
    };
    let groups = if process.supplementary_groups.is_empty() {
        String::new()
    } else {
        process
            .supplementary_groups
            .iter()
            .map(|group| format!("{group}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let content = format!(
        concat!(
            "Name:\t{}\n",
            "Umask:\t0000\n",
            "State:\t{}\n",
            "Tgid:\t{}\n",
            "Pid:\t{}\n",
            "PPid:\t{}\n",
            "TracerPid:\t0\n",
            "Uid:\t{}\t{}\t{}\t{}\n",
            "Gid:\t{}\t{}\t{}\t{}\n",
            "FDSize:\t{}\n",
            "Groups:\t{}\n",
            "CapInh:\t{}\n",
            "CapPrm:\t{}\n",
            "CapEff:\t{}\n",
            "CapBnd:\t{}\n",
            "CapAmb:\t{}\n",
            "NoNewPrivs:\t0\n",
            "Seccomp:\t0\n",
            "Seccomp_filters:\t0\n",
        ),
        pid_string(pid),
        state,
        pid.0,
        pid.0,
        parent_pid,
        process.real_uid,
        process.effective_uid,
        process.saved_uid,
        process.fs_uid,
        process.real_gid,
        process.effective_gid,
        process.saved_gid,
        process.fs_gid,
        process.objects.len().max(64),
        groups,
        format_capability_set(
            process.capability_inheritable[0],
            process.capability_inheritable[1],
        ),
        format_capability_set(
            process.capability_permitted[0],
            process.capability_permitted[1]
        ),
        format_capability_set(
            process.capability_effective[0],
            process.capability_effective[1]
        ),
        format_capability_set(
            process.capability_permitted[0],
            process.capability_permitted[1]
        ),
        format_capability_set(process.capability_ambient[0], process.capability_ambient[1]),
    );
    Ok(content.into_bytes())
}

pub(super) fn proc_pid_uid_map_bytes(pid: ProcessID) -> FSResult<Vec<u8>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    Ok(process
        .user_namespace_uid_map
        .clone()
        .unwrap_or_else(|| default_user_namespace_map(process.real_uid))
        .into_bytes())
}

pub(super) fn proc_pid_gid_map_bytes(pid: ProcessID) -> FSResult<Vec<u8>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    Ok(process
        .user_namespace_gid_map
        .clone()
        .unwrap_or_else(|| default_user_namespace_map(process.real_gid))
        .into_bytes())
}

pub(super) fn proc_pid_setgroups_bytes(pid: ProcessID) -> FSResult<Vec<u8>> {
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    let process = process.lock();
    Ok(process
        .user_namespace_setgroups
        .clone()
        .unwrap_or_else(|| String::from("allow\n"))
        .into_bytes())
}

pub(super) fn proc_pid_write_uid_map(pid: ProcessID, buffer: &[u8]) -> FSResult<usize> {
    let value = normalize_proc_control_write(buffer)?;
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    process.lock().user_namespace_uid_map = Some(value);
    Ok(buffer.len())
}

pub(super) fn proc_pid_write_gid_map(pid: ProcessID, buffer: &[u8]) -> FSResult<usize> {
    let value = normalize_proc_control_write(buffer)?;
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    process.lock().user_namespace_gid_map = Some(value);
    Ok(buffer.len())
}

pub(super) fn proc_pid_write_setgroups(pid: ProcessID, buffer: &[u8]) -> FSResult<usize> {
    let value = normalize_proc_control_write(buffer)?;
    let process = get_process_with_pid(pid).map_err(|_| FSError::NotFound)?;
    process.lock().user_namespace_setgroups = Some(value);
    Ok(buffer.len())
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

    let mut content = String::from("pos:\t0\nflags:\t0\nmnt_id:\t0\nino:\t0\n");
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

pub(super) fn pid_comm_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 2
}

pub(super) fn pid_mountinfo_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 3
}

pub(super) fn pid_cgroup_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 4
}

pub(super) fn pid_oom_score_adj_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 5
}

pub(super) fn pid_stat_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 6
}

pub(super) fn pid_status_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 7
}

pub(super) fn pid_uid_map_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 8
}

pub(super) fn pid_gid_map_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 9
}

pub(super) fn pid_setgroups_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 10
}

pub(super) fn pid_root_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 11
}

pub(super) fn pid_environ_inode(pid: ProcessID) -> u64 {
    pid_dir_inode(pid) + 12
}

pub(super) fn pid_fd_dir_inode(pid: ProcessID) -> u64 {
    0x5000_0000 + pid.0 * 0x100
}

pub(super) fn pid_fdinfo_dir_inode(pid: ProcessID) -> u64 {
    0x5100_0000 + pid.0 * 0x100
}

pub(super) fn pid_ns_dir_inode(pid: ProcessID) -> u64 {
    0x5200_0000 + pid.0 * 0x100
}

pub(super) fn pid_fd_inode(pid: ProcessID, fd: &str) -> u64 {
    pid_fd_dir_inode(pid) + 1 + fd.parse::<u64>().unwrap_or(0)
}

pub(super) fn pid_fdinfo_inode(pid: ProcessID, fd: &str) -> u64 {
    pid_fdinfo_dir_inode(pid) + 1 + fd.parse::<u64>().unwrap_or(0)
}

pub(super) fn pid_ns_inode(pid: ProcessID, name: &str) -> FSResult<u64> {
    let _ = pid;
    match name {
        "cgroup" => Ok(PROC_CGROUP_INIT_INO),
        "ipc" => Ok(PROC_IPC_INIT_INO),
        "mnt" => Ok(PROC_MNT_INIT_INO),
        "net" => Ok(PROC_NET_INIT_INO),
        "pid" => Ok(PROC_PID_INIT_INO),
        "time" => Ok(PROC_TIME_INIT_INO),
        "user" => Ok(PROC_USER_INIT_INO),
        "uts" => Ok(PROC_UTS_INIT_INO),
        _ => Err(FSError::NotFound),
    }
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
