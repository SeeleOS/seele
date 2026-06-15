use super::*;

#[derive(Clone)]
pub(super) struct PathLookup {
    pub(super) stat: LinuxStat,
    pub(super) mount_id: u64,
    pub(super) mount_root: bool,
}

#[derive(Clone, Copy)]
pub(super) struct PathLookupPhases {
    pub(super) resolve: HotSyscallPhase,
    pub(super) empty_path: HotSyscallPhase,
    pub(super) resolve_final: HotSyscallPhase,
    pub(super) build_stat: HotSyscallPhase,
    pub(super) mount_info: HotSyscallPhase,
}
pub(super) fn linux_stat_from_file_like_info(info: FileLikeInfo, path: &Path) -> LinuxStat {
    let rdev = info.rdev;
    let mut stat = info.as_linux();
    stat.st_dev = mount_device_id_for_path(path);
    stat.st_rdev = rdev;
    stat
}

pub(super) fn mount_info_from_object(object: &ObjectRef) -> Result<(u64, bool), SyscallError> {
    Ok((mount_id_for_object(object)?, mount_root_for_object(object)?))
}

pub(super) fn lookup_path_metadata(
    dirfd: i32,
    path_str: &str,
    nofollow: bool,
    allow_empty_path: bool,
    phases: PathLookupPhases,
) -> Result<PathLookup, SyscallError> {
    if path_str.is_empty() && allow_empty_path {
        let empty_path_start = profile::scope_start();
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        profile::record_hot_syscall_phase(
            phases.empty_path,
            profile::scope_start().saturating_sub(empty_path_start),
        );

        let build_stat_start = profile::scope_start();
        let stat = object.clone().as_statable()?.stat();
        profile::record_hot_syscall_phase(
            phases.build_stat,
            profile::scope_start().saturating_sub(build_stat_start),
        );

        let mount_info_start = profile::scope_start();
        let (mount_id, mount_root) = mount_info_from_object(&object)?;
        profile::record_hot_syscall_phase(
            phases.mount_info,
            profile::scope_start().saturating_sub(mount_info_start),
        );
        return Ok(PathLookup {
            stat,
            mount_id,
            mount_root,
        });
    }

    let resolve_start = profile::scope_start();
    let normalized_path = resolve_path_at(dirfd, path_str)?.normalize();
    profile::record_hot_syscall_phase(
        phases.resolve,
        profile::scope_start().saturating_sub(resolve_start),
    );

    let resolve_final_start = profile::scope_start();
    let (info, resolved_path, mount_id, mount_root) =
        resolve_path_with_mount_info(normalized_path, !nofollow)?;
    profile::record_hot_syscall_phase(
        phases.resolve_final,
        profile::scope_start().saturating_sub(resolve_final_start),
    );

    let build_stat_start = profile::scope_start();
    let stat = linux_stat_from_file_like_info(info, &resolved_path);
    profile::record_hot_syscall_phase(
        phases.build_stat,
        profile::scope_start().saturating_sub(build_stat_start),
    );

    Ok(PathLookup {
        stat,
        mount_id,
        mount_root,
    })
}
pub(super) fn check_access_mode(mode: i32) -> Result<(), SyscallError> {
    if (mode & !7) != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(())
}

pub(super) fn check_access_permissions(stat: &LinuxStat, mode: i32) -> Result<(), SyscallError> {
    let permission = stat.st_mode & 0o777;

    if (mode & 4) != 0 && permission & 0o444 == 0 {
        return Err(SyscallError::AccessDenied);
    }
    if (mode & 2) != 0 && permission & 0o222 == 0 {
        return Err(SyscallError::AccessDenied);
    }
    if (mode & 1) != 0 && permission & 0o111 == 0 {
        return Err(SyscallError::AccessDenied);
    }

    Ok(())
}

pub(super) fn linux_major(dev: u64) -> u32 {
    (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)) as u32
}

pub(super) fn linux_minor(dev: u64) -> u32 {
    ((dev & 0xff) | ((dev >> 12) & !0xff)) as u32
}

pub(super) fn filesystem_magic_for_file_like(
    file_like: &FileLikeObject,
) -> Result<i64, SyscallError> {
    filesystem_magic_for_path(&file_like.path())
}

pub(super) fn filesystem_magic_for_object(object: &ObjectRef) -> Result<i64, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return filesystem_magic_for_file_like(&file_like);
    }

    if object.clone().as_pidfd().is_ok()
        || object.clone().as_eventfd().is_ok()
        || object.clone().as_inotify().is_ok()
        || object.clone().as_poller().is_ok()
        || object.clone().as_signalfd().is_ok()
        || object.clone().as_timerfd().is_ok()
    {
        return Ok(ANON_INODE_FS_MAGIC);
    }

    if object.clone().as_inet_socket().is_ok()
        || object.clone().as_netlink_socket().is_ok()
        || object.clone().as_unix_socket().is_ok()
    {
        return Ok(SOCKFS_MAGIC);
    }

    Err(SyscallError::BadFileDescriptor)
}

pub(super) fn filesystem_magic_for_path(path: &Path) -> Result<i64, SyscallError> {
    let fs = {
        let (_mount_path, fs, _, _) = VirtualFS.lock().mount_metadata(path.clone())?;
        fs
    };
    Ok(fs.lock().magic())
}

pub(super) fn mount_id_for_file_like(file_like: &FileLikeObject) -> Result<u64, SyscallError> {
    Ok(file_like.mount_id())
}

pub(super) fn pseudo_mount_id(magic: i64) -> Option<u64> {
    let offset = match magic {
        SOCKFS_MAGIC => 0,
        ANON_INODE_FS_MAGIC => 1,
        _ => return None,
    };

    let mount_count = VirtualFS.lock().mount_count() as u64;
    Some(mount_count + 1 + offset)
}

pub(super) fn mount_id_for_object(object: &ObjectRef) -> Result<u64, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return mount_id_for_file_like(&file_like);
    }

    let magic = filesystem_magic_for_object(object)?;
    pseudo_mount_id(magic).ok_or(SyscallError::BadFileDescriptor)
}

pub(super) fn mount_root_for_object(object: &ObjectRef) -> Result<bool, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return Ok(file_like.mount_root());
    }

    let magic = filesystem_magic_for_object(object)?;
    if pseudo_mount_id(magic).is_some() {
        return Ok(false);
    }

    Err(SyscallError::BadFileDescriptor)
}

pub(super) fn stat_mount_id_at(
    dirfd: i32,
    path_str: &str,
    flags: AtFlags,
) -> Result<u64, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return mount_id_for_object(&object);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    Ok(VirtualFS.lock().mount_id(path)?)
}

pub(super) fn linux_statfs(f_type: i64) -> LinuxStatFs {
    LinuxStatFs {
        f_type,
        f_bsize: 4096,
        f_blocks: 262_144,
        f_bfree: 131_072,
        f_bavail: 131_072,
        f_files: 262_144,
        f_ffree: 131_072,
        f_fsid: 1,
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    }
}
pub(super) fn faccessat_impl(
    dirfd: i32,
    path_str: &str,
    mode: i32,
    flags: AtFlags,
) -> Result<usize, SyscallError> {
    let allowed = (AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW | AtFlags::EACCESS).bits();
    if flags.bits() != flags.bits() & allowed {
        return Err(SyscallError::NoSyscall);
    }

    check_access_mode(mode)?;

    if path_str.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }

        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        check_access_permissions(&object.as_statable()?.stat(), mode)?;
        return Ok(0);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let open_result = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path.clone())
    } else {
        open_path(path.clone())
    };
    let object: ObjectRef = Arc::new(open_result?);
    check_access_permissions(&object.as_statable()?.stat(), mode)?;
    Ok(0)
}
pub(super) fn stat_at(
    dirfd: i32,
    path_str: &str,
    flags: AtFlags,
) -> Result<LinuxStat, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return Ok(object.as_statable()?.stat());
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let open_result = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path.clone())
    } else {
        open_path(path.clone())
    };
    let object: ObjectRef = Arc::new(open_result?);
    let stat = object.as_statable()?.stat();
    Ok(stat)
}

pub(super) fn chmod_at(
    dirfd: i32,
    path_str: &str,
    mode: u32,
    flags: AtFlags,
) -> Result<usize, SyscallError> {
    let allowed_flags = AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW;
    if flags.bits() != (flags & allowed_flags).bits() {
        return Err(SyscallError::InvalidArguments);
    }

    let mode = mode & !S_IFMT;
    if path_str.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }
        chmod_fd_object(
            get_object_current_process(dirfd as u64).map_err(SyscallError::from)?,
            mode,
        )?;
        return Ok(0);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let file = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path)?
    } else {
        open_path(path)?
    };
    if flags.contains(AtFlags::SYMLINK_NOFOLLOW)
        && matches!(file.info()?.file_like_type, FileLikeType::Symlink)
    {
        return Err(SyscallError::OperationNotSupported);
    }

    file.chmod(mode)?;
    Ok(0)
}

pub(super) fn chmod_fd_object(object: ObjectRef, mode: u32) -> Result<(), SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        file_like.chmod(mode)?;
    } else {
        let _ = object.as_statable()?;
    }

    Ok(())
}

pub(super) fn chown_at(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<usize, SyscallError> {
    let allowed_flags = AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT;
    if flags.bits() != (flags & allowed_flags).bits() {
        return Err(SyscallError::InvalidArguments);
    }

    if path_str.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }
        chown_fd_object(get_object_current_process(dirfd as u64).map_err(SyscallError::from)?)?;
        return Ok(0);
    }

    ensure_path_exists_at(dirfd, path_str, flags.contains(AtFlags::SYMLINK_NOFOLLOW))?;
    Ok(0)
}

pub(super) fn chown_fd_object(object: ObjectRef) -> Result<(), SyscallError> {
    if object.clone().as_file_like().is_err() {
        let _ = object.as_statable()?;
    }

    Ok(())
}
