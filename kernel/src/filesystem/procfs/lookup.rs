use super::*;

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
        ["filesystems"] => Ok(proc_file(
            "filesystems",
            PROC_FILESYSTEMS_INODE,
            proc_filesystems_bytes,
        )),
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
        ["sys", "fs", "inotify"] => Ok(proc_dir(
            "/sys/fs/inotify",
            "inotify",
            PROC_SYS_FS_INOTIFY_INODE,
            proc_fs_inotify_entries(),
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
        ["sys", "kernel", "pid_max"] => Ok(proc_rw_file(
            "pid_max",
            PROC_SYS_KERNEL_PID_MAX_INODE,
            || proc_sysctl_value_bytes(&PROC_PID_MAX),
            |buffer| proc_write_sysctl_u64(&PROC_PID_MAX, buffer),
        )),
        ["sys", "kernel", "cap_last_cap"] => Ok(proc_file(
            "cap_last_cap",
            PROC_SYS_KERNEL_CAP_LAST_CAP_INODE,
            proc_cap_last_cap_bytes,
        )),
        ["sys", "kernel", "tainted"] => Ok(proc_file(
            "tainted",
            PROC_SYS_KERNEL_TAINTED_INODE,
            proc_tainted_bytes,
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
        ["sys", "fs", "inotify", "max_queued_events"] => Ok(proc_rw_file(
            "max_queued_events",
            PROC_SYS_FS_INOTIFY_MAX_QUEUED_EVENTS_INODE,
            || proc_sysctl_value_bytes(&PROC_INOTIFY_MAX_QUEUED_EVENTS),
            |buffer| proc_write_sysctl_u64(&PROC_INOTIFY_MAX_QUEUED_EVENTS, buffer),
        )),
        ["sys", "fs", "inotify", "max_user_instances"] => Ok(proc_rw_file(
            "max_user_instances",
            PROC_SYS_FS_INOTIFY_MAX_USER_INSTANCES_INODE,
            || proc_sysctl_value_bytes(&PROC_INOTIFY_MAX_USER_INSTANCES),
            |buffer| proc_write_sysctl_u64(&PROC_INOTIFY_MAX_USER_INSTANCES, buffer),
        )),
        ["sys", "fs", "inotify", "max_user_watches"] => Ok(proc_rw_file(
            "max_user_watches",
            PROC_SYS_FS_INOTIFY_MAX_USER_WATCHES_INODE,
            || proc_sysctl_value_bytes(&PROC_INOTIFY_MAX_USER_WATCHES),
            |buffer| proc_write_sysctl_u64(&PROC_INOTIFY_MAX_USER_WATCHES, buffer),
        )),
        ["sys", "fs", "nr_open"] => Ok(proc_rw_file(
            "nr_open",
            PROC_SYS_FS_NR_OPEN_INODE,
            || proc_sysctl_value_bytes(&PROC_NR_OPEN),
            |buffer| proc_write_sysctl_u64(&PROC_NR_OPEN, buffer),
        )),
        ["self", ..] => lookup_proc_self_path(&parts),
        [pid, ..] if parse_pid(pid).is_ok() => lookup_proc_pid_path(&parts),
        _ => Err(FSError::NotFound),
    }
}
