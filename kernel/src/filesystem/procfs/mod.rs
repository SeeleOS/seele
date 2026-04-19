use alloc::format;

use crate::filesystem::{
    errors::FSError,
    path::{Path, PathPart},
    vfs::FSResult,
    vfs_traits::{FileLike, FileSystem},
};

mod nodes;
mod pid;
mod root;

use nodes::{proc_dir, proc_file, proc_rw_file, proc_symlink};
use pid::{
    current_pid, ensure_pid_exists, fd_target, parse_fd, parse_pid, pid_cgroup_inode,
    pid_cmdline_inode, pid_dir_entries, pid_dir_inode, pid_fd_dir_inode, pid_fd_entries,
    pid_fd_inode, pid_fdinfo_dir_inode, pid_fdinfo_entries, pid_fdinfo_inode, pid_mountinfo_inode,
    pid_oom_score_adj_inode, pid_stat_inode, pid_string, proc_pid_cgroup_bytes,
    proc_pid_cmdline_bytes, proc_pid_fdinfo_bytes, proc_pid_oom_score_adj_bytes,
    proc_pid_stat_bytes, proc_pid_write_oom_score_adj,
};
use root::{
    PROC_CMDLINE_INODE, PROC_MOUNTS_INODE, PROC_ROOT_INODE, PROC_SELF_INODE,
    proc_kernel_cmdline_bytes, proc_mountinfo_bytes, proc_mounts_bytes, proc_root_entries,
};

pub struct ProcFs;

impl ProcFs {
    pub fn new() -> Self {
        Self
    }
}

impl FileSystem for ProcFs {
    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        let normalized = path.normalize();
        let parts = normalized
            .parts
            .iter()
            .filter_map(|part| match part {
                PathPart::Normal(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<alloc::vec::Vec<_>>();

        match parts.as_slice() {
            [] => Ok(proc_dir("/", PROC_ROOT_INODE, proc_root_entries())),
            ["cmdline"] => Ok(proc_file(
                "cmdline",
                PROC_CMDLINE_INODE,
                proc_kernel_cmdline_bytes,
            )),
            ["mounts"] => Ok(proc_file("mounts", PROC_MOUNTS_INODE, proc_mounts_bytes)),
            ["self"] => {
                let pid = current_pid()?;
                Ok(proc_symlink("self", PROC_SELF_INODE, format!("{}", pid.0)))
            }
            ["self", "cmdline"] => {
                let pid = current_pid()?;
                Ok(proc_file("cmdline", pid_cmdline_inode(pid), move || {
                    proc_pid_cmdline_bytes(pid)
                }))
            }
            ["self", "stat"] => {
                let pid = current_pid()?;
                Ok(proc_file("stat", pid_stat_inode(pid), move || {
                    proc_pid_stat_bytes(pid).unwrap_or_default()
                }))
            }
            ["self", "cgroup"] => {
                let pid = current_pid()?;
                Ok(proc_file("cgroup", pid_cgroup_inode(pid), move || {
                    proc_pid_cgroup_bytes(pid)
                }))
            }
            ["self", "oom_score_adj"] => {
                let pid = current_pid()?;
                Ok(proc_rw_file(
                    "oom_score_adj",
                    pid_oom_score_adj_inode(pid),
                    move || proc_pid_oom_score_adj_bytes(pid).unwrap_or_default(),
                    move |buffer| proc_pid_write_oom_score_adj(pid, buffer),
                ))
            }
            ["self", "mountinfo"] => {
                let pid = current_pid()?;
                Ok(proc_file(
                    "mountinfo",
                    pid_mountinfo_inode(pid),
                    proc_mountinfo_bytes,
                ))
            }
            ["self", "fd"] => {
                let pid = current_pid()?;
                Ok(proc_dir("fd", pid_fd_dir_inode(pid), pid_fd_entries(pid)?))
            }
            ["self", "fd", fd] => {
                let pid = current_pid()?;
                let fd = parse_fd(fd)?;
                Ok(proc_symlink(fd, pid_fd_inode(pid, fd), fd_target(pid, fd)?))
            }
            ["self", "fdinfo"] => {
                let pid = current_pid()?;
                Ok(proc_dir(
                    "fdinfo",
                    pid_fdinfo_dir_inode(pid),
                    pid_fdinfo_entries(pid)?,
                ))
            }
            ["self", "fdinfo", fd] => {
                let pid = current_pid()?;
                let fd = parse_fd(fd)?;
                let fd_num = fd.parse::<usize>().map_err(|_| FSError::NotFound)?;
                Ok(proc_file("fdinfo", pid_fdinfo_inode(pid, fd), move || {
                    proc_pid_fdinfo_bytes(pid, fd_num).unwrap_or_default()
                }))
            }
            [pid] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_dir(
                    pid_string(pid).as_str(),
                    pid_dir_inode(pid),
                    pid_dir_entries(),
                ))
            }
            [pid, "cmdline"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_file("cmdline", pid_cmdline_inode(pid), move || {
                    proc_pid_cmdline_bytes(pid)
                }))
            }
            [pid, "stat"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_file("stat", pid_stat_inode(pid), move || {
                    proc_pid_stat_bytes(pid).unwrap_or_default()
                }))
            }
            [pid, "cgroup"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_file("cgroup", pid_cgroup_inode(pid), move || {
                    proc_pid_cgroup_bytes(pid)
                }))
            }
            [pid, "oom_score_adj"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_rw_file(
                    "oom_score_adj",
                    pid_oom_score_adj_inode(pid),
                    move || proc_pid_oom_score_adj_bytes(pid).unwrap_or_default(),
                    move |buffer| proc_pid_write_oom_score_adj(pid, buffer),
                ))
            }
            [pid, "mountinfo"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_file(
                    "mountinfo",
                    pid_mountinfo_inode(pid),
                    proc_mountinfo_bytes,
                ))
            }
            [pid, "fd"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_dir("fd", pid_fd_dir_inode(pid), pid_fd_entries(pid)?))
            }
            [pid, "fd", fd] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                let fd = parse_fd(fd)?;
                Ok(proc_symlink(fd, pid_fd_inode(pid, fd), fd_target(pid, fd)?))
            }
            [pid, "fdinfo"] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                Ok(proc_dir(
                    "fdinfo",
                    pid_fdinfo_dir_inode(pid),
                    pid_fdinfo_entries(pid)?,
                ))
            }
            [pid, "fdinfo", fd] => {
                let pid = parse_pid(pid)?;
                ensure_pid_exists(pid)?;
                let fd = parse_fd(fd)?;
                let fd_num = fd.parse::<usize>().map_err(|_| FSError::NotFound)?;
                Ok(proc_file("fdinfo", pid_fdinfo_inode(pid, fd), move || {
                    proc_pid_fdinfo_bytes(pid, fd_num).unwrap_or_default()
                }))
            }
            _ => Err(FSError::NotFound),
        }
    }

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn name(&self) -> &'static str {
        "proc"
    }

    fn magic(&self) -> i64 {
        0x9fa0
    }

    fn mount_source(&self) -> &'static str {
        "proc"
    }

    fn mount_options(&self, _path: &Path) -> &'static str {
        "rw,nosuid,nodev,noexec,relatime"
    }
}
