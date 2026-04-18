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

use nodes::{proc_dir, proc_file, proc_symlink};
use pid::{
    current_pid, ensure_pid_exists, fd_target, parse_fd, parse_pid, pid_cmdline_inode,
    pid_dir_entries, pid_dir_inode, pid_fd_dir_inode, pid_fd_entries, pid_fd_inode, pid_string,
    proc_pid_cmdline_bytes,
};
use root::{
    PROC_CMDLINE_INODE, PROC_ROOT_INODE, PROC_SELF_INODE, proc_kernel_cmdline_bytes,
    proc_root_entries,
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
            ["self", "fd"] => {
                let pid = current_pid()?;
                Ok(proc_dir("fd", pid_fd_dir_inode(pid), pid_fd_entries(pid)?))
            }
            ["self", "fd", fd] => {
                let pid = current_pid()?;
                let fd = parse_fd(fd)?;
                Ok(proc_symlink(fd, pid_fd_inode(pid, fd), fd_target(pid, fd)?))
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
            _ => Err(FSError::NotFound),
        }
    }
}
