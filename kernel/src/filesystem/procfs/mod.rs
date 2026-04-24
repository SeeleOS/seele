use alloc::format;

use alloc::{vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::filesystem::{
    errors::FSError,
    info::DirectoryContentInfo,
    path::{Path, PathPart},
    vfs::FSResult,
    vfs_traits::{DirectoryContentType, FileLike, FileSystem},
};

mod nodes;
mod pid;
mod root;

use nodes::{proc_dir, proc_file, proc_rw_file, proc_symlink};
use pid::{
    current_pid, ensure_pid_exists, fd_target, parse_fd, parse_pid, pid_cgroup_inode,
    pid_cmdline_inode, pid_comm_inode, pid_dir_entries, pid_dir_inode, pid_environ_inode,
    pid_fd_dir_inode, pid_fd_entries, pid_fd_inode, pid_fdinfo_dir_inode, pid_fdinfo_entries,
    pid_fdinfo_inode, pid_gid_map_inode, pid_mountinfo_inode, pid_ns_dir_inode, pid_ns_entries,
    pid_ns_inode, pid_oom_score_adj_inode, pid_root_inode, pid_setgroups_inode, pid_stat_inode,
    pid_status_inode, pid_string, pid_uid_map_inode, proc_pid_cgroup_bytes, proc_pid_cmdline_bytes,
    proc_pid_comm_bytes, proc_pid_environ_bytes, proc_pid_fdinfo_bytes, proc_pid_gid_map_bytes,
    proc_pid_oom_score_adj_bytes, proc_pid_setgroups_bytes, proc_pid_stat_bytes,
    proc_pid_status_bytes, proc_pid_uid_map_bytes, proc_pid_write_gid_map,
    proc_pid_write_oom_score_adj, proc_pid_write_setgroups, proc_pid_write_uid_map,
};
use root::{
    PROC_CMDLINE_INODE, PROC_DEVICES_INODE, PROC_MEMINFO_INODE, PROC_MOUNTS_INODE,
    PROC_PRESSURE_CPU_INODE, PROC_PRESSURE_INODE, PROC_PRESSURE_IO_INODE,
    PROC_PRESSURE_MEMORY_INODE, PROC_ROOT_INODE, PROC_SELF_INODE, PROC_SYS_FS_FILE_MAX_INODE,
    PROC_SYS_FS_INODE, PROC_SYS_FS_NR_OPEN_INODE, PROC_SYS_INODE, PROC_SYS_KERNEL_DOMAINNAME_INODE,
    PROC_SYS_KERNEL_HOSTNAME_INODE, PROC_SYS_KERNEL_INODE, PROC_SYS_KERNEL_OSRELEASE_INODE,
    PROC_SYS_KERNEL_RANDOM_BOOT_ID_INODE, PROC_SYS_KERNEL_RANDOM_INODE,
    PROC_SYS_KERNEL_RANDOM_UUID_INODE, proc_boot_id_bytes, proc_devices_bytes,
    proc_kernel_cmdline_bytes, proc_kernel_entries, proc_kernel_random_entries,
    proc_mountinfo_bytes, proc_mounts_bytes, proc_pressure_entries, proc_random_uuid_bytes,
    proc_root_entries,
};

const DEFAULT_FILE_MAX: u64 = 1_048_576;
const DEFAULT_NR_OPEN: u64 = 1_048_576;

static PROC_FILE_MAX: AtomicU64 = AtomicU64::new(DEFAULT_FILE_MAX);
static PROC_NR_OPEN: AtomicU64 = AtomicU64::new(DEFAULT_NR_OPEN);

fn proc_hostname_bytes() -> Vec<u8> {
    proc_c_string_bytes(crate::misc::utsname::current_hostname(crate::NAME))
}

fn proc_domainname_bytes() -> Vec<u8> {
    proc_c_string_bytes(crate::misc::utsname::current_domainname("(none)"))
}

fn proc_osrelease_bytes() -> Vec<u8> {
    format!("{}\n", crate::misc::utsname::DEFAULT_RELEASE).into_bytes()
}

fn proc_meminfo_bytes() -> Vec<u8> {
    let total_kib = crate::memory::usable_memory_bytes() / 1024;
    format!(
        concat!(
            "MemTotal:       {:>8} kB\n",
            "MemFree:        {:>8} kB\n",
            "MemAvailable:   {:>8} kB\n",
            "Buffers:        {:>8} kB\n",
            "Cached:         {:>8} kB\n",
            "SwapCached:     {:>8} kB\n",
            "Active:         {:>8} kB\n",
            "Inactive:       {:>8} kB\n",
            "Active(anon):   {:>8} kB\n",
            "Inactive(anon): {:>8} kB\n",
            "Active(file):   {:>8} kB\n",
            "Inactive(file): {:>8} kB\n",
            "Unevictable:    {:>8} kB\n",
            "Mlocked:        {:>8} kB\n",
            "SwapTotal:      {:>8} kB\n",
            "SwapFree:       {:>8} kB\n"
        ),
        total_kib, total_kib, total_kib, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    )
    .into_bytes()
}

fn proc_pressure_bytes() -> Vec<u8> {
    b"some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"
        .to_vec()
}

fn proc_write_pressure(buffer: &[u8]) -> FSResult<usize> {
    // systemd programs PSI triggers via writes to /proc/pressure/*.
    // We do not implement real PSI accounting yet, but accepting the
    // trigger string matches the expected userspace setup flow.
    Ok(buffer.len())
}

fn proc_write_hostname(buffer: &[u8]) -> FSResult<usize> {
    let value = proc_trim_sysctl_string(buffer)?;
    crate::misc::utsname::set_hostname(value.as_bytes()).map_err(|_| FSError::Other)?;
    Ok(buffer.len())
}

fn proc_write_domainname(buffer: &[u8]) -> FSResult<usize> {
    let value = proc_trim_sysctl_string(buffer)?;
    crate::misc::utsname::set_domainname(value.as_bytes()).map_err(|_| FSError::Other)?;
    Ok(buffer.len())
}

fn proc_c_string_bytes(value: [u8; 65]) -> Vec<u8> {
    let len = value
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(value.len());
    let mut bytes = value[..len].to_vec();
    bytes.push(b'\n');
    bytes
}

fn proc_trim_sysctl_string(buffer: &[u8]) -> FSResult<&str> {
    core::str::from_utf8(buffer)
        .map(|value| value.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0'))
        .map_err(|_| FSError::Other)
}

fn proc_fs_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("file-max".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("nr_open".into(), DirectoryContentType::File),
    ]
}

fn proc_sys_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("fs".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("kernel".into(), DirectoryContentType::Directory),
    ]
}

fn proc_sysctl_value_bytes(value: &AtomicU64) -> Vec<u8> {
    format!("{}\n", value.load(Ordering::Relaxed)).into_bytes()
}

fn proc_write_sysctl_u64(target: &AtomicU64, buffer: &[u8]) -> FSResult<usize> {
    let content = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
    let value = content
        .trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0')
        .parse::<u64>()
        .map_err(|_| FSError::Other)?;
    target.store(value, Ordering::Relaxed);
    Ok(buffer.len())
}

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

pub(super) fn lookup_proc_path(path: &Path) -> FSResult<FileLike> {
    let normalized = path.normalize();
    let parts = normalized
        .parts
        .iter()
        .filter_map(|part| match part {
            PathPart::Normal(name) => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    match parts.as_slice() {
        [] => Ok(proc_dir("/", "/", PROC_ROOT_INODE, proc_root_entries())),
        ["cmdline"] => Ok(proc_file(
            "cmdline",
            PROC_CMDLINE_INODE,
            proc_kernel_cmdline_bytes,
        )),
        ["devices"] => Ok(proc_file("devices", PROC_DEVICES_INODE, proc_devices_bytes)),
        ["meminfo"] => Ok(proc_file("meminfo", PROC_MEMINFO_INODE, proc_meminfo_bytes)),
        ["mounts"] => Ok(proc_file("mounts", PROC_MOUNTS_INODE, proc_mounts_bytes)),
        ["pressure"] => Ok(proc_dir(
            "/pressure",
            "pressure",
            PROC_PRESSURE_INODE,
            proc_pressure_entries(),
        )),
        ["pressure", "cpu"] => Ok(proc_rw_file(
            "cpu",
            PROC_PRESSURE_CPU_INODE,
            proc_pressure_bytes,
            proc_write_pressure,
        )),
        ["pressure", "io"] => Ok(proc_rw_file(
            "io",
            PROC_PRESSURE_IO_INODE,
            proc_pressure_bytes,
            proc_write_pressure,
        )),
        ["pressure", "memory"] => Ok(proc_rw_file(
            "memory",
            PROC_PRESSURE_MEMORY_INODE,
            proc_pressure_bytes,
            proc_write_pressure,
        )),
        ["sys"] => Ok(proc_dir("/sys", "sys", PROC_SYS_INODE, proc_sys_entries())),
        ["sys", "fs"] => Ok(proc_dir(
            "/sys/fs",
            "fs",
            PROC_SYS_FS_INODE,
            proc_fs_entries(),
        )),
        ["sys", "kernel"] => Ok(proc_dir(
            "/sys/kernel",
            "kernel",
            PROC_SYS_KERNEL_INODE,
            proc_kernel_entries(),
        )),
        ["sys", "kernel", "hostname"] => Ok(proc_rw_file(
            "hostname",
            PROC_SYS_KERNEL_HOSTNAME_INODE,
            proc_hostname_bytes,
            proc_write_hostname,
        )),
        ["sys", "kernel", "domainname"] => Ok(proc_rw_file(
            "domainname",
            PROC_SYS_KERNEL_DOMAINNAME_INODE,
            proc_domainname_bytes,
            proc_write_domainname,
        )),
        ["sys", "kernel", "osrelease"] => Ok(proc_file(
            "osrelease",
            PROC_SYS_KERNEL_OSRELEASE_INODE,
            proc_osrelease_bytes,
        )),
        ["sys", "kernel", "random"] => Ok(proc_dir(
            "/sys/kernel/random",
            "random",
            PROC_SYS_KERNEL_RANDOM_INODE,
            proc_kernel_random_entries(),
        )),
        ["sys", "kernel", "random", "boot_id"] => Ok(proc_file(
            "boot_id",
            PROC_SYS_KERNEL_RANDOM_BOOT_ID_INODE,
            proc_boot_id_bytes,
        )),
        ["sys", "kernel", "random", "uuid"] => Ok(proc_file(
            "uuid",
            PROC_SYS_KERNEL_RANDOM_UUID_INODE,
            proc_random_uuid_bytes,
        )),
        ["sys", "fs", "file-max"] => Ok(proc_rw_file(
            "file-max",
            PROC_SYS_FS_FILE_MAX_INODE,
            || proc_sysctl_value_bytes(&PROC_FILE_MAX),
            |buffer| proc_write_sysctl_u64(&PROC_FILE_MAX, buffer),
        )),
        ["sys", "fs", "nr_open"] => Ok(proc_rw_file(
            "nr_open",
            PROC_SYS_FS_NR_OPEN_INODE,
            || proc_sysctl_value_bytes(&PROC_NR_OPEN),
            |buffer| proc_write_sysctl_u64(&PROC_NR_OPEN, buffer),
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
        ["self", "comm"] => {
            let pid = current_pid()?;
            Ok(proc_file("comm", pid_comm_inode(pid), move || {
                proc_pid_comm_bytes(pid).unwrap_or_default()
            }))
        }
        ["self", "environ"] => {
            let pid = current_pid()?;
            Ok(proc_file("environ", pid_environ_inode(pid), move || {
                proc_pid_environ_bytes(pid).unwrap_or_default()
            }))
        }
        ["self", "stat"] => {
            let pid = current_pid()?;
            Ok(proc_file("stat", pid_stat_inode(pid), move || {
                proc_pid_stat_bytes(pid).unwrap_or_default()
            }))
        }
        ["self", "status"] => {
            let pid = current_pid()?;
            Ok(proc_file("status", pid_status_inode(pid), move || {
                proc_pid_status_bytes(pid).unwrap_or_default()
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
        ["self", "uid_map"] => {
            let pid = current_pid()?;
            Ok(proc_rw_file(
                "uid_map",
                pid_uid_map_inode(pid),
                move || proc_pid_uid_map_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_uid_map(pid, buffer),
            ))
        }
        ["self", "gid_map"] => {
            let pid = current_pid()?;
            Ok(proc_rw_file(
                "gid_map",
                pid_gid_map_inode(pid),
                move || proc_pid_gid_map_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_gid_map(pid, buffer),
            ))
        }
        ["self", "setgroups"] => {
            let pid = current_pid()?;
            Ok(proc_rw_file(
                "setgroups",
                pid_setgroups_inode(pid),
                move || proc_pid_setgroups_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_setgroups(pid, buffer),
            ))
        }
        ["self", "root"] => {
            let pid = current_pid()?;
            Ok(proc_symlink("root", pid_root_inode(pid), "/".into()))
        }
        ["self", "ns"] => {
            let pid = current_pid()?;
            Ok(proc_dir(
                "/self/ns",
                "ns",
                pid_ns_dir_inode(pid),
                pid_ns_entries(),
            ))
        }
        ["self", "ns", namespace] => {
            let pid = current_pid()?;
            Ok(proc_file(
                namespace,
                pid_ns_inode(pid, namespace)?,
                Vec::new,
            ))
        }
        ["self", "fd"] => {
            let pid = current_pid()?;
            Ok(proc_dir(
                "/self/fd",
                "fd",
                pid_fd_dir_inode(pid),
                pid_fd_entries(pid)?,
            ))
        }
        ["self", "fd", fd] => {
            let pid = current_pid()?;
            let fd = parse_fd(fd)?;
            Ok(proc_symlink(fd, pid_fd_inode(pid, fd), fd_target(pid, fd)?))
        }
        ["self", "fdinfo"] => {
            let pid = current_pid()?;
            Ok(proc_dir(
                "/self/fdinfo",
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
                &alloc::format!("/{}", pid.0),
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
        [pid, "comm"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_file("comm", pid_comm_inode(pid), move || {
                proc_pid_comm_bytes(pid).unwrap_or_default()
            }))
        }
        [pid, "environ"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_file("environ", pid_environ_inode(pid), move || {
                proc_pid_environ_bytes(pid).unwrap_or_default()
            }))
        }
        [pid, "stat"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_file("stat", pid_stat_inode(pid), move || {
                proc_pid_stat_bytes(pid).unwrap_or_default()
            }))
        }
        [pid, "status"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_file("status", pid_status_inode(pid), move || {
                proc_pid_status_bytes(pid).unwrap_or_default()
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
        [pid, "uid_map"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_rw_file(
                "uid_map",
                pid_uid_map_inode(pid),
                move || proc_pid_uid_map_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_uid_map(pid, buffer),
            ))
        }
        [pid, "gid_map"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_rw_file(
                "gid_map",
                pid_gid_map_inode(pid),
                move || proc_pid_gid_map_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_gid_map(pid, buffer),
            ))
        }
        [pid, "setgroups"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_rw_file(
                "setgroups",
                pid_setgroups_inode(pid),
                move || proc_pid_setgroups_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_setgroups(pid, buffer),
            ))
        }
        [pid, "root"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_symlink("root", pid_root_inode(pid), "/".into()))
        }
        [pid, "ns"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_dir(
                &alloc::format!("/{}/ns", pid.0),
                "ns",
                pid_ns_dir_inode(pid),
                pid_ns_entries(),
            ))
        }
        [pid, "ns", namespace] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_file(
                namespace,
                pid_ns_inode(pid, namespace)?,
                Vec::new,
            ))
        }
        [pid, "fd"] => {
            let pid = parse_pid(pid)?;
            ensure_pid_exists(pid)?;
            Ok(proc_dir(
                &alloc::format!("/{}/fd", pid.0),
                "fd",
                pid_fd_dir_inode(pid),
                pid_fd_entries(pid)?,
            ))
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
                &alloc::format!("/{}/fdinfo", pid.0),
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

impl FileSystem for ProcFs {
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
