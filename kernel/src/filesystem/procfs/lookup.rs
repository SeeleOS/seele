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
        ["cpuinfo"] => Ok(proc_file("cpuinfo", PROC_CPUINFO_INODE, proc_cpuinfo_bytes)),
        ["devices"] => Ok(proc_file("devices", PROC_DEVICES_INODE, proc_devices_bytes)),
        ["filesystems"] => Ok(proc_file(
            "filesystems",
            PROC_FILESYSTEMS_INODE,
            proc_filesystems_bytes,
        )),
        ["kpageflags"] => Ok(proc_sparse_file(
            "kpageflags",
            PROC_KPAGEFLAGS_INODE,
            proc_kpageflags_read_at,
        )),
        ["key-users"] => Ok(proc_file(
            "key-users",
            PROC_KEY_USERS_INODE,
            crate::systemcall::implementations::proc_key_users_bytes,
        )),
        ["loadavg"] => Ok(proc_file("loadavg", PROC_LOADAVG_INODE, proc_loadavg_bytes)),
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
        ["version"] => Ok(proc_file("version", PROC_VERSION_INODE, proc_version_bytes)),
        ["pressure"] => Ok(proc_dir(
            "/pressure",
            "pressure",
            PROC_PRESSURE_INODE,
            proc_pressure_entries(),
        )),
        ["pressure", "cpu"] => Ok(proc_rw_file_with_epoll(
            "cpu",
            PROC_PRESSURE_CPU_INODE,
            proc_pressure_bytes,
            proc_write_pressure,
            true,
        )),
        ["pressure", "io"] => Ok(proc_rw_file_with_epoll(
            "io",
            PROC_PRESSURE_IO_INODE,
            proc_pressure_bytes,
            proc_write_pressure,
            true,
        )),
        ["pressure", "memory"] => Ok(proc_rw_file_with_epoll(
            "memory",
            PROC_PRESSURE_MEMORY_INODE,
            proc_pressure_bytes,
            proc_write_pressure,
            true,
        )),
        ["sysvipc"] => Ok(proc_dir(
            "/sysvipc",
            "sysvipc",
            PROC_SYSVIPC_INODE,
            proc_sysvipc_entries(),
        )),
        ["sysvipc", "msg"] => Ok(proc_file(
            "msg",
            PROC_SYSVIPC_MSG_INODE,
            crate::ipc::sysv_msg::proc_sysvipc_msg_bytes,
        )),
        ["sysvipc", "sem"] => Ok(proc_file(
            "sem",
            PROC_SYSVIPC_SEM_INODE,
            crate::ipc::sysv_sem::proc_sysvipc_sem_bytes,
        )),
        ["sysvipc", "shm"] => Ok(proc_file(
            "shm",
            PROC_SYSVIPC_SHM_INODE,
            crate::ipc::sysv_shm::proc_sysvipc_shm_bytes,
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
        ["sys", "net"] => Ok(proc_dir(
            "/sys/net",
            "net",
            PROC_SYS_INODE + 0x100,
            proc_sys_net_entries(),
        )),
        ["sys", "net", "ipv4"] => Ok(proc_dir(
            "/sys/net/ipv4",
            "ipv4",
            PROC_SYS_INODE + 0x101,
            proc_sys_net_ipv4_entries(),
        )),
        ["sys", "net", "ipv4", "conf"] => Ok(proc_dir(
            "/sys/net/ipv4/conf",
            "conf",
            PROC_SYS_INODE + 0x102,
            proc_sys_net_ipv4_conf_entries(),
        )),
        ["sys", "net", "ipv4", "conf", "lo"] => Ok(proc_dir(
            "/sys/net/ipv4/conf/lo",
            "lo",
            PROC_SYS_INODE + 0x103,
            proc_sys_net_ipv4_conf_if_entries(),
        )),
        ["sys", "net", "ipv4", "conf", "default"] => Ok(proc_dir(
            "/sys/net/ipv4/conf/default",
            "default",
            PROC_SYS_INODE + 0x104,
            proc_sys_net_ipv4_conf_if_entries(),
        )),
        ["sys", "vm"] => Ok(proc_dir(
            "/sys/vm",
            "vm",
            PROC_SYS_VM_INODE,
            proc_vm_entries(),
        )),
        ["sys", "vm", "drop_caches"] => Ok(proc_rw_file(
            "drop_caches",
            PROC_SYS_VM_DROP_CACHES_INODE,
            proc_drop_caches_bytes,
            proc_write_drop_caches,
        )),
        ["sys", "vm", "compact_memory"] => Ok(proc_rw_file(
            "compact_memory",
            PROC_SYS_VM_COMPACT_MEMORY_INODE,
            || b"0\n".to_vec(),
            proc_write_drop_caches,
        )),
        ["sys", "vm", "nr_hugepages"] => Ok(proc_rw_file(
            "nr_hugepages",
            PROC_SYS_VM_NR_HUGEPAGES_INODE,
            || proc_sysctl_value_bytes(&PROC_NR_HUGEPAGES),
            |buffer| proc_write_sysctl_u64(&PROC_NR_HUGEPAGES, buffer),
        )),
        ["sys", "vm", "hugetlb_shm_group"] => Ok(proc_rw_file(
            "hugetlb_shm_group",
            PROC_SYS_VM_HUGETLB_SHM_GROUP_INODE,
            || proc_sysctl_value_bytes(&PROC_HUGETLB_SHM_GROUP),
            |buffer| proc_write_sysctl_u64(&PROC_HUGETLB_SHM_GROUP, buffer),
        )),
        ["sys", "vm", "overcommit_memory"] => Ok(proc_rw_file(
            "overcommit_memory",
            PROC_SYS_VM_OVERCOMMIT_MEMORY_INODE,
            || proc_sysctl_value_bytes(&PROC_OVERCOMMIT_MEMORY),
            |buffer| proc_write_sysctl_u64(&PROC_OVERCOMMIT_MEMORY, buffer),
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
        ["sys", "net", "ipv4", "conf", "lo", "tag"] => Ok(proc_rw_file(
            "tag",
            PROC_SYS_INODE + 0x105,
            proc_net_ipv4_conf_lo_tag_bytes,
            proc_write_net_ipv4_conf_lo_tag,
        )),
        ["sys", "net", "ipv4", "conf", "default", "tag"] => Ok(proc_rw_file(
            "tag",
            PROC_SYS_INODE + 0x106,
            proc_net_ipv4_conf_default_tag_bytes,
            proc_write_net_ipv4_conf_default_tag,
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
        ["sys", "fs", "pipe-max-size"] => Ok(proc_rw_file(
            "pipe-max-size",
            PROC_SYS_FS_PIPE_MAX_SIZE_INODE,
            || proc_sysctl_value_bytes(&PROC_PIPE_MAX_SIZE),
            |buffer| proc_write_sysctl_u64(&PROC_PIPE_MAX_SIZE, buffer),
        )),
        ["self", ..] => lookup_proc_self_path(&parts),
        [pid, ..] if parse_pid(pid).is_ok() => lookup_proc_pid_path(&parts),
        _ => Err(FSError::NotFound),
    }
}
