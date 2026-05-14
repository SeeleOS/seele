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

mod net;
mod nodes;
mod pid;
mod root;

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
use root::{
    PROC_CMDLINE_INODE, PROC_DEVICES_INODE, PROC_MEMINFO_INODE, PROC_MOUNTS_INODE,
    PROC_PRESSURE_CPU_INODE, PROC_PRESSURE_INODE, PROC_PRESSURE_IO_INODE,
    PROC_PRESSURE_MEMORY_INODE, PROC_ROOT_INODE, PROC_SELF_INODE, PROC_STAT_INODE,
    PROC_SYS_FS_FILE_MAX_INODE, PROC_SYS_FS_INODE, PROC_SYS_FS_NR_OPEN_INODE, PROC_SYS_INODE,
    PROC_SYS_KERNEL_CAP_LAST_CAP_INODE, PROC_SYS_KERNEL_DOMAINNAME_INODE,
    PROC_SYS_KERNEL_HOSTNAME_INODE, PROC_SYS_KERNEL_INODE, PROC_SYS_KERNEL_NGROUPS_MAX_INODE,
    PROC_SYS_KERNEL_OSRELEASE_INODE, PROC_SYS_KERNEL_RANDOM_BOOT_ID_INODE,
    PROC_SYS_KERNEL_RANDOM_INODE, PROC_SYS_KERNEL_RANDOM_UUID_INODE, PROC_UPTIME_INODE,
    proc_boot_id_bytes, proc_cap_last_cap_bytes, proc_devices_bytes, proc_kernel_cmdline_bytes,
    proc_kernel_entries, proc_kernel_random_entries, proc_mountinfo_bytes, proc_mounts_bytes,
    proc_ngroups_max_bytes, proc_pressure_entries, proc_random_uuid_bytes, proc_root_entries,
    proc_stat_bytes, proc_uptime_bytes,
};

const DEFAULT_FILE_MAX: u64 = 1_048_576;
const DEFAULT_NR_OPEN: u64 = 1_048_576;

static PROC_FILE_MAX: AtomicU64 = AtomicU64::new(DEFAULT_FILE_MAX);
static PROC_NR_OPEN: AtomicU64 = AtomicU64::new(DEFAULT_NR_OPEN);

fn proc_pid_namespace_file(
    pid: crate::process::misc::ProcessID,
    namespace: &str,
) -> FSResult<FileLike> {
    let inode = pid_ns_inode(pid, namespace)?;
    if let Some(object) = pid_ns_object(pid, namespace)? {
        return Ok(proc_object_file(namespace, inode, object));
    }

    Ok(proc_file(namespace, inode, Vec::new))
}

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
        [] => Ok(proc_dynamic_dir(
            "/",
            "/",
            PROC_ROOT_INODE,
            proc_root_entries,
        )),
        ["cmdline"] => Ok(proc_file(
            "cmdline",
            PROC_CMDLINE_INODE,
            proc_kernel_cmdline_bytes,
        )),
        ["devices"] => Ok(proc_file("devices", PROC_DEVICES_INODE, proc_devices_bytes)),
        ["meminfo"] => Ok(proc_file("meminfo", PROC_MEMINFO_INODE, proc_meminfo_bytes)),
        ["mounts"] => Ok(proc_file("mounts", PROC_MOUNTS_INODE, proc_mounts_bytes)),
        ["net"] => Ok(proc_dir("/net", "net", PROC_NET_INODE, proc_net_entries())),
        ["net", "dev"] => Ok(proc_file("dev", PROC_NET_DEV_INODE, proc_net_dev_bytes)),
        ["net", "route"] => Ok(proc_file(
            "route",
            PROC_NET_ROUTE_INODE,
            proc_net_route_bytes,
        )),
        ["net", "if_inet6"] => Ok(proc_file(
            "if_inet6",
            PROC_NET_IF_INET6_INODE,
            proc_net_if_inet6_bytes,
        )),
        ["stat"] => Ok(proc_file("stat", PROC_STAT_INODE, proc_stat_bytes)),
        ["uptime"] => Ok(proc_file("uptime", PROC_UPTIME_INODE, proc_uptime_bytes)),
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
        ["sys", "kernel", "ngroups_max"] => Ok(proc_file(
            "ngroups_max",
            PROC_SYS_KERNEL_NGROUPS_MAX_INODE,
            proc_ngroups_max_bytes,
        )),
        ["sys", "kernel", "cap_last_cap"] => Ok(proc_file(
            "cap_last_cap",
            PROC_SYS_KERNEL_CAP_LAST_CAP_INODE,
            proc_cap_last_cap_bytes,
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
        ["self", "sessionid"] => {
            let pid = current_pid()?;
            Ok(proc_file(
                "sessionid",
                pid_sessionid_inode(pid),
                move || proc_pid_sessionid_bytes(pid).unwrap_or_default(),
            ))
        }
        ["self", "loginuid"] => {
            let pid = current_pid()?;
            Ok(proc_file("loginuid", pid_loginuid_inode(pid), move || {
                proc_pid_loginuid_bytes(pid).unwrap_or_default()
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
        ["self", "net"] => Ok(proc_dir(
            "/self/net",
            "net",
            PROC_NET_INODE,
            proc_net_entries(),
        )),
        ["self", "net", "dev"] => Ok(proc_file("dev", PROC_NET_DEV_INODE, proc_net_dev_bytes)),
        ["self", "net", "route"] => Ok(proc_file(
            "route",
            PROC_NET_ROUTE_INODE,
            proc_net_route_bytes,
        )),
        ["self", "net", "if_inet6"] => Ok(proc_file(
            "if_inet6",
            PROC_NET_IF_INET6_INODE,
            proc_net_if_inet6_bytes,
        )),
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
            proc_pid_namespace_file(pid, namespace)
        }
        ["self", "fd"] => {
            let pid = current_pid()?;
            Ok(proc_dynamic_dir(
                "/self/fd",
                "fd",
                pid_fd_dir_inode(pid),
                move || pid_fd_entries(pid).unwrap_or_default(),
            ))
        }
        ["self", "fd", fd] => {
            let pid = current_pid()?;
            let fd = parse_fd(fd)?;
            let fd_name = String::from(fd);
            Ok(proc_dynamic_symlink(fd, pid_fd_inode(pid, fd), move || {
                fd_target(pid, &fd_name)
            }))
        }
        ["self", "fdinfo"] => {
            let pid = current_pid()?;
            Ok(proc_dynamic_dir(
                "/self/fdinfo",
                "fdinfo",
                pid_fdinfo_dir_inode(pid),
                move || pid_fdinfo_entries(pid).unwrap_or_default(),
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
            Ok(proc_dir(
                &alloc::format!("/{}", pid.0),
                pid_string(pid).as_str(),
                pid_dir_inode(pid),
                pid_dir_entries(),
            ))
        }
        [pid, "cmdline"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file("cmdline", pid_cmdline_inode(pid), move || {
                proc_pid_cmdline_bytes(pid)
            }))
        }
        [pid, "comm"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file("comm", pid_comm_inode(pid), move || {
                proc_pid_comm_bytes(pid).unwrap_or_default()
            }))
        }
        [pid, "environ"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file("environ", pid_environ_inode(pid), move || {
                proc_pid_environ_bytes(pid).unwrap_or_default()
            }))
        }
        [pid, "stat"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file("stat", pid_stat_inode(pid), move || {
                proc_pid_stat_bytes(pid).unwrap_or_default()
            }))
        }
        [pid, "status"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file("status", pid_status_inode(pid), move || {
                proc_pid_status_bytes(pid).unwrap_or_default()
            }))
        }
        [pid, "sessionid"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file(
                "sessionid",
                pid_sessionid_inode(pid),
                move || proc_pid_sessionid_bytes(pid).unwrap_or_default(),
            ))
        }
        [pid, "loginuid"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file("loginuid", pid_loginuid_inode(pid), move || {
                proc_pid_loginuid_bytes(pid).unwrap_or_default()
            }))
        }
        [pid, "cgroup"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file("cgroup", pid_cgroup_inode(pid), move || {
                proc_pid_cgroup_bytes(pid)
            }))
        }
        [pid, "oom_score_adj"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_rw_file(
                "oom_score_adj",
                pid_oom_score_adj_inode(pid),
                move || proc_pid_oom_score_adj_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_oom_score_adj(pid, buffer),
            ))
        }
        [pid, "mountinfo"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file(
                "mountinfo",
                pid_mountinfo_inode(pid),
                proc_mountinfo_bytes,
            ))
        }
        [pid, "uid_map"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_rw_file(
                "uid_map",
                pid_uid_map_inode(pid),
                move || proc_pid_uid_map_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_uid_map(pid, buffer),
            ))
        }
        [pid, "gid_map"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_rw_file(
                "gid_map",
                pid_gid_map_inode(pid),
                move || proc_pid_gid_map_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_gid_map(pid, buffer),
            ))
        }
        [pid, "setgroups"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_rw_file(
                "setgroups",
                pid_setgroups_inode(pid),
                move || proc_pid_setgroups_bytes(pid).unwrap_or_default(),
                move |buffer| proc_pid_write_setgroups(pid, buffer),
            ))
        }
        [pid, "root"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_symlink("root", pid_root_inode(pid), "/".into()))
        }
        [pid, "net"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_dir(
                &format!("/{}/net", pid.0),
                "net",
                PROC_NET_INODE,
                proc_net_entries(),
            ))
        }
        [pid, "net", "dev"] => {
            parse_pid(pid)?;
            Ok(proc_file("dev", PROC_NET_DEV_INODE, proc_net_dev_bytes))
        }
        [pid, "net", "route"] => {
            parse_pid(pid)?;
            Ok(proc_file(
                "route",
                PROC_NET_ROUTE_INODE,
                proc_net_route_bytes,
            ))
        }
        [pid, "net", "if_inet6"] => {
            parse_pid(pid)?;
            Ok(proc_file(
                "if_inet6",
                PROC_NET_IF_INET6_INODE,
                proc_net_if_inet6_bytes,
            ))
        }
        [pid, "ns"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_dir(
                &alloc::format!("/{}/ns", pid.0),
                "ns",
                pid_ns_dir_inode(pid),
                pid_ns_entries(),
            ))
        }
        [pid, "ns", namespace] => {
            let pid = parse_pid(pid)?;
            proc_pid_namespace_file(pid, namespace)
        }
        [pid, "fd"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_dynamic_dir(
                &alloc::format!("/{}/fd", pid.0),
                "fd",
                pid_fd_dir_inode(pid),
                move || pid_fd_entries(pid).unwrap_or_default(),
            ))
        }
        [pid, "fd", fd] => {
            let pid = parse_pid(pid)?;
            let fd = parse_fd(fd)?;
            let fd_name = String::from(fd);
            Ok(proc_dynamic_symlink(fd, pid_fd_inode(pid, fd), move || {
                fd_target(pid, &fd_name)
            }))
        }
        [pid, "fdinfo"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_dynamic_dir(
                &alloc::format!("/{}/fdinfo", pid.0),
                "fdinfo",
                pid_fdinfo_dir_inode(pid),
                move || pid_fdinfo_entries(pid).unwrap_or_default(),
            ))
        }
        [pid, "fdinfo", fd] => {
            let pid = parse_pid(pid)?;
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

#[cfg(test)]
mod tests {
    use super::{
        proc_c_string_bytes, proc_fs_entries, proc_pressure_bytes, proc_sys_entries,
        proc_sysctl_value_bytes, proc_trim_sysctl_string, proc_write_domainname,
        proc_write_hostname, proc_write_pressure, proc_write_sysctl_u64,
    };
    use crate::filesystem::errors::FSError;
    use crate::misc::utsname::{
        current_domainname, current_hostname, set_domainname, set_hostname,
    };
    use core::sync::atomic::AtomicU64;

    crate::test!(
        procfs_string_helpers,
        "procfs string helpers trim sysctl values and preserve c-string bytes",
        procfs_string_helpers_trim_sysctl_values_and_preserve_c_string_bytes
    );
    crate::test!(
        procfs_static_entry_sets,
        "procfs static entry builders expose stable names",
        procfs_static_entry_builders_expose_stable_names
    );
    crate::test!(
        procfs_pressure_and_sysctl_bytes,
        "procfs pressure and sysctl rendering stay stable",
        procfs_pressure_and_sysctl_rendering_stays_stable
    );
    crate::test!(
        procfs_write_helpers,
        "procfs write helpers trim values update state and reject invalid inputs",
        procfs_write_helpers_trim_values_update_state_and_reject_invalid_inputs
    );

    fn procfs_string_helpers_trim_sysctl_values_and_preserve_c_string_bytes() {
        assert_eq!(proc_trim_sysctl_string(b" host \n\0").unwrap(), "host");
        assert!(matches!(
            proc_trim_sysctl_string(&[0xff]),
            Err(FSError::Other)
        ));

        let mut field = [0u8; 65];
        field[..4].copy_from_slice(b"host");
        assert_eq!(proc_c_string_bytes(field), b"host\n");
    }

    fn procfs_static_entry_builders_expose_stable_names() {
        let fs_entries = proc_fs_entries();
        assert_eq!(fs_entries.len(), 2);
        assert_eq!(fs_entries[0].name, "file-max");
        assert_eq!(fs_entries[1].name, "nr_open");

        let sys_entries = proc_sys_entries();
        assert_eq!(sys_entries.len(), 2);
        assert_eq!(sys_entries[0].name, "fs");
        assert_eq!(sys_entries[1].name, "kernel");
    }

    fn procfs_pressure_and_sysctl_rendering_stays_stable() {
        let rendered = proc_pressure_bytes();
        assert!(
            core::str::from_utf8(&rendered)
                .unwrap()
                .contains("some avg10=0.00")
        );
        let value = AtomicU64::new(1234);
        assert_eq!(proc_sysctl_value_bytes(&value), b"1234\n");
    }

    fn procfs_write_helpers_trim_values_update_state_and_reject_invalid_inputs() {
        let hostname_before = current_hostname(crate::NAME);
        let domain_before = current_domainname("(none)");

        assert_eq!(proc_write_hostname(b" proc-host \n").unwrap(), 12);
        assert_eq!(
            proc_c_string_bytes(current_hostname(crate::NAME)),
            b"proc-host\n"
        );
        assert!(matches!(proc_write_hostname(&[0xff]), Err(FSError::Other)));
        set_hostname(
            proc_trim_sysctl_string(&proc_c_string_bytes(hostname_before))
                .unwrap()
                .as_bytes(),
        )
        .unwrap();

        assert_eq!(proc_write_domainname(b" domain.test \n").unwrap(), 14);
        assert_eq!(
            proc_c_string_bytes(current_domainname("(none)")),
            b"domain.test\n"
        );
        assert!(matches!(
            proc_write_domainname(&[0xff]),
            Err(FSError::Other)
        ));
        set_domainname(
            proc_trim_sysctl_string(&proc_c_string_bytes(domain_before))
                .unwrap()
                .as_bytes(),
        )
        .unwrap();

        let value = AtomicU64::new(7);
        assert_eq!(proc_write_sysctl_u64(&value, b" 123 \0").unwrap(), 6);
        assert_eq!(proc_sysctl_value_bytes(&value), b"123\n");
        assert!(matches!(
            proc_write_sysctl_u64(&value, b"not-a-number"),
            Err(FSError::Other)
        ));

        assert_eq!(proc_write_pressure(b"some 100 1000").unwrap(), 13);
    }
}
