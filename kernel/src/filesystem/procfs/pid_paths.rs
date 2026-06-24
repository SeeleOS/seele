use super::*;

pub(super) fn lookup_proc_pid_path(parts: &[&str]) -> FSResult<FileLike> {
    match parts {
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
            Ok(proc_file_with_epoll(
                "mountinfo",
                pid_mountinfo_inode(pid),
                proc_mountinfo_bytes,
                true,
            ))
        }
        [pid, "mounts"] => {
            parse_pid(pid)?;
            Ok(proc_file("mounts", PROC_MOUNTS_INODE, proc_mounts_bytes))
        }
        [pid, "maps"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_file("maps", pid_maps_inode(pid), move || {
                proc_pid_maps_bytes(pid).unwrap_or_default()
            }))
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
        [pid, "exe"] => {
            let pid = parse_pid(pid)?;
            Ok(proc_dynamic_symlink("exe", pid_exe_inode(pid), move || {
                proc_pid_exe_target(pid)
            }))
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
