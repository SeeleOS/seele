use alloc::format;

use alloc::{string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::filesystem::{
    errors::FSError,
    info::DirectoryContentInfo,
    path::{Path, PathPart},
    vfs::FSResult,
    vfs_traits::{DirectoryContentType, FileLike, FileSystem},
};

mod lookup;
mod net;
mod nodes;
mod pid;
mod pid_paths;
mod root;
mod self_paths;
mod sysctl;

#[cfg(test)]
mod sysctl_tests;

use lookup::lookup_proc_path;
use net::{
    PROC_NET_DEV_INODE, PROC_NET_IF_INET6_INODE, PROC_NET_INODE, PROC_NET_ROUTE_INODE,
    proc_net_dev_bytes, proc_net_entries, proc_net_if_inet6_bytes, proc_net_route_bytes,
};
use nodes::{
    proc_dir, proc_dynamic_dir, proc_dynamic_symlink, proc_file, proc_object_file, proc_rw_file,
    proc_symlink,
};
use pid::{
    current_pid, fd_target, parse_fd, parse_pid, pid_cgroup_inode, pid_cmdline_inode,
    pid_comm_inode, pid_dir_entries, pid_dir_inode, pid_environ_inode, pid_fd_dir_inode,
    pid_fd_entries, pid_fd_inode, pid_fdinfo_dir_inode, pid_fdinfo_entries, pid_fdinfo_inode,
    pid_gid_map_inode, pid_loginuid_inode, pid_mountinfo_inode, pid_ns_dir_inode, pid_ns_entries,
    pid_ns_inode, pid_ns_object, pid_oom_score_adj_inode, pid_root_inode, pid_sessionid_inode,
    pid_setgroups_inode, pid_stat_inode, pid_status_inode, pid_string, pid_uid_map_inode,
    proc_pid_cgroup_bytes, proc_pid_cmdline_bytes, proc_pid_comm_bytes, proc_pid_environ_bytes,
    proc_pid_fdinfo_bytes, proc_pid_gid_map_bytes, proc_pid_loginuid_bytes,
    proc_pid_oom_score_adj_bytes, proc_pid_sessionid_bytes, proc_pid_setgroups_bytes,
    proc_pid_stat_bytes, proc_pid_status_bytes, proc_pid_uid_map_bytes, proc_pid_write_gid_map,
    proc_pid_write_oom_score_adj, proc_pid_write_setgroups, proc_pid_write_uid_map,
};
use pid_paths::lookup_proc_pid_path;
use root::{
    PROC_CMDLINE_INODE, PROC_DEVICES_INODE, PROC_MEMINFO_INODE, PROC_MOUNTS_INODE,
    PROC_PRESSURE_CPU_INODE, PROC_PRESSURE_INODE, PROC_PRESSURE_IO_INODE,
    PROC_PRESSURE_MEMORY_INODE, PROC_ROOT_INODE, PROC_SELF_INODE, PROC_STAT_INODE,
    PROC_SYS_FS_FILE_MAX_INODE, PROC_SYS_FS_INODE, PROC_SYS_FS_INOTIFY_INODE,
    PROC_SYS_FS_INOTIFY_MAX_QUEUED_EVENTS_INODE, PROC_SYS_FS_INOTIFY_MAX_USER_INSTANCES_INODE,
    PROC_SYS_FS_INOTIFY_MAX_USER_WATCHES_INODE, PROC_SYS_FS_NR_OPEN_INODE, PROC_SYS_INODE,
    PROC_SYS_KERNEL_CAP_LAST_CAP_INODE, PROC_SYS_KERNEL_DOMAINNAME_INODE,
    PROC_SYS_KERNEL_HOSTNAME_INODE, PROC_SYS_KERNEL_INODE, PROC_SYS_KERNEL_NGROUPS_MAX_INODE,
    PROC_SYS_KERNEL_OSRELEASE_INODE, PROC_SYS_KERNEL_RANDOM_BOOT_ID_INODE,
    PROC_SYS_KERNEL_RANDOM_INODE, PROC_SYS_KERNEL_RANDOM_UUID_INODE, PROC_UPTIME_INODE,
    proc_boot_id_bytes, proc_cap_last_cap_bytes, proc_devices_bytes, proc_kernel_cmdline_bytes,
    proc_kernel_entries, proc_kernel_random_entries, proc_mountinfo_bytes, proc_mounts_bytes,
    proc_ngroups_max_bytes, proc_pressure_entries, proc_random_uuid_bytes, proc_root_entries,
    proc_stat_bytes, proc_uptime_bytes,
};
use self_paths::{lookup_proc_self_path, proc_pid_namespace_file};
use sysctl::*;

pub struct ProcFs;

impl ProcFs {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProcFs {
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for ProcFs {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn init(&mut self) -> FSResult<()> {
        Ok(())
    }

    fn lookup(&self, path: &Path) -> FSResult<FileLike> {
        lookup_proc_path(path)
    }

    fn rename(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
        Err(FSError::Readonly)
    }

    fn link(&self, _old_path: &Path, _new_path: &Path) -> FSResult<()> {
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

    fn default_mount_flags(&self, _path: &Path) -> crate::filesystem::vfs_traits::MountFlags {
        crate::filesystem::vfs_traits::MountFlags::MS_NOSUID
            | crate::filesystem::vfs_traits::MountFlags::MS_NODEV
            | crate::filesystem::vfs_traits::MountFlags::MS_NOEXEC
            | crate::filesystem::vfs_traits::MountFlags::MS_RELATIME
    }
}
