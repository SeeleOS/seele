use super::*;

pub(super) fn proc_pid_namespace_file(
    pid: crate::process::misc::ProcessID,
    namespace: &str,
) -> FSResult<FileLike> {
    let inode = pid_ns_inode(pid, namespace)?;
    if let Some(object) = pid_ns_object(pid, namespace)? {
        return Ok(proc_object_file(namespace, inode, object));
    }

    Ok(proc_file(namespace, inode, Vec::new))
}

pub(super) fn lookup_proc_self_path(parts: &[&str]) -> FSResult<FileLike> {
    match parts {
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
            Ok(proc_file_with_epoll(
                "mountinfo",
                pid_mountinfo_inode(pid),
                proc_mountinfo_bytes,
                true,
            ))
        }
        ["self", "mounts"] => Ok(proc_file("mounts", PROC_MOUNTS_INODE, proc_mounts_bytes)),
        ["self", "maps"] => {
            let pid = current_pid()?;
            Ok(proc_file("maps", pid_maps_inode(pid), move || {
                proc_pid_maps_bytes(pid).unwrap_or_default()
            }))
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
        ["self", "exe"] => {
            let pid = current_pid()?;
            Ok(proc_dynamic_symlink("exe", pid_exe_inode(pid), move || {
                proc_pid_exe_target(pid)
            }))
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
        _ => Err(FSError::NotFound),
    }
}
