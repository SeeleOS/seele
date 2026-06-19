use super::*;

define_syscall!(Fstat, |fd: u64, linux_stat_ptr: *mut LinuxStat| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    let stat = object.as_statable()?.stat();
    user_safe::write(linux_stat_ptr, &stat)?;
    Ok(0)
});

define_syscall!(Stat, |path: CString, linux_stat_ptr: *mut LinuxStat| {
    let path_str = path_from_raw(path)?;
    let lookup = lookup_path_metadata(
        AT_FDCWD,
        &path_str,
        false,
        false,
        PathLookupPhases {
            resolve: HotSyscallPhase::NewfstatatPathResolve,
            empty_path: HotSyscallPhase::NewfstatatEmptyPath,
            resolve_final: HotSyscallPhase::NewfstatatResolveFinal,
            build_stat: HotSyscallPhase::NewfstatatBuildStat,
            mount_info: HotSyscallPhase::NewfstatatMountInfo,
        },
    )?;

    let write_user_start = profile::scope_start();
    user_safe::write(linux_stat_ptr, &lookup.stat)?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::NewfstatatWriteUser,
        profile::scope_start().saturating_sub(write_user_start),
    );
    Ok(0)
});

define_syscall!(Lstat, |path: CString, linux_stat_ptr: *mut LinuxStat| {
    let path_str = path_from_raw(path)?;
    let lookup = lookup_path_metadata(
        AT_FDCWD,
        &path_str,
        true,
        false,
        PathLookupPhases {
            resolve: HotSyscallPhase::NewfstatatPathResolve,
            empty_path: HotSyscallPhase::NewfstatatEmptyPath,
            resolve_final: HotSyscallPhase::NewfstatatResolveFinal,
            build_stat: HotSyscallPhase::NewfstatatBuildStat,
            mount_info: HotSyscallPhase::NewfstatatMountInfo,
        },
    )?;

    let write_user_start = profile::scope_start();
    user_safe::write(linux_stat_ptr, &lookup.stat)?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::NewfstatatWriteUser,
        profile::scope_start().saturating_sub(write_user_start),
    );
    Ok(0)
});

define_syscall!(Fchmod, |fd: u64, mode: u32| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    let mode = mode & !S_IFMT;
    chmod_fd_object(object, mode)?;
    Ok(0)
});

define_syscall!(Fchmodat, |dirfd: i32, path: CString, mode: u32| {
    let path_str = path_from_raw(path)?;
    chmod_at(dirfd, &path_str, mode, AtFlags::empty())
});

define_syscall!(Fchown, |fd: u64, owner: u32, group: u32| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    chown_fd_object(object, owner, group)?;
    Ok(0)
});

define_syscall!(Fchmodat2, |dirfd: i32,
                            path: u64,
                            mode: u32,
                            flags: AtFlags| {
    let path = path as CString;
    let path_str = if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            String::new()
        } else {
            return Err(SyscallError::BadAddress);
        }
    } else {
        path_from_raw(path)?
    };

    chmod_at(dirfd, &path_str, mode, flags)
});

define_syscall!(Fchownat, |dirfd: i32,
                           path: u64,
                           owner: u32,
                           group: u32,
                           flags: AtFlags| {
    let path = path as CString;
    let path_str = if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            String::new()
        } else {
            return Err(SyscallError::BadAddress);
        }
    } else {
        path_from_raw(path)?
    };

    chown_at(dirfd, &path_str, owner, group, flags)
});

define_syscall!(Newfstatat, |dirfd: i32,
                             path: u64,
                             linux_stat_ptr: *mut LinuxStat,
                             flags: AtFlags| {
    let path = path as CString;
    if flags.bits()
        != flags.bits()
            & (AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT | AtFlags::EMPTY_PATH).bits()
    {
        return Err(SyscallError::InvalidArguments);
    }
    let path_str = if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            String::new()
        } else {
            return Err(SyscallError::BadAddress);
        }
    } else {
        path_from_raw(path)?
    };

    let lookup = lookup_path_metadata(
        dirfd,
        &path_str,
        flags.contains(AtFlags::SYMLINK_NOFOLLOW),
        flags.contains(AtFlags::EMPTY_PATH),
        PathLookupPhases {
            resolve: HotSyscallPhase::NewfstatatPathResolve,
            empty_path: HotSyscallPhase::NewfstatatEmptyPath,
            resolve_final: HotSyscallPhase::NewfstatatResolveFinal,
            build_stat: HotSyscallPhase::NewfstatatBuildStat,
            mount_info: HotSyscallPhase::NewfstatatMountInfo,
        },
    )?;

    let write_user_start = profile::scope_start();
    user_safe::write(linux_stat_ptr, &lookup.stat)?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::NewfstatatWriteUser,
        profile::scope_start().saturating_sub(write_user_start),
    );
    Ok(0)
});

define_syscall!(Statx, |dirfd: i32,
                        path: CString,
                        flags: AtFlags,
                        _mask: u32,
                        statx_ptr: *mut LinuxStatx| {
    let allowed_flags = (AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT | AtFlags::EMPTY_PATH)
        .bits()
        | AT_STATX_FORCE_SYNC
        | AT_STATX_DONT_SYNC;
    if flags.bits() != flags.bits() & allowed_flags {
        return Err(SyscallError::InvalidArguments);
    }
    if flags.contains(AtFlags::STATX_FORCE_SYNC) && flags.contains(AtFlags::STATX_DONT_SYNC) {
        return Err(SyscallError::InvalidArguments);
    }
    let path_str = if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            String::new()
        } else {
            return Err(SyscallError::BadAddress);
        }
    } else {
        path_from_raw(path)?
    };
    if statx_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let lookup = lookup_path_metadata(
        dirfd,
        &path_str,
        flags.contains(AtFlags::SYMLINK_NOFOLLOW),
        flags.contains(AtFlags::EMPTY_PATH),
        PathLookupPhases {
            resolve: HotSyscallPhase::StatxPathResolve,
            empty_path: HotSyscallPhase::StatxEmptyPath,
            resolve_final: HotSyscallPhase::StatxResolveFinal,
            build_stat: HotSyscallPhase::StatxBuildStat,
            mount_info: HotSyscallPhase::StatxMountInfo,
        },
    )?;
    let stat = lookup.stat;
    let mount_id = lookup.mount_id;
    let mount_root = lookup.mount_root;

    let pack_output_start = profile::scope_start();
    let statx = LinuxStatx {
        stx_mask: STATX_BASIC_STATS | STATX_MNT_ID,
        stx_blksize: stat.st_blksize as u32,
        stx_attributes: if mount_root { STATX_ATTR_MOUNT_ROOT } else { 0 },
        stx_nlink: stat.st_nlink as u32,
        stx_uid: stat.st_uid,
        stx_gid: stat.st_gid,
        stx_mode: stat.st_mode as u16,
        stx_ino: stat.st_ino,
        stx_size: stat.st_size as u64,
        stx_blocks: stat.st_blocks as u64,
        stx_atime: StatxTimestamp {
            tv_sec: stat.st_atime,
            tv_nsec: stat.st_atime_nsec as u32,
            __reserved: 0,
        },
        stx_ctime: StatxTimestamp {
            tv_sec: stat.st_ctime,
            tv_nsec: stat.st_ctime_nsec as u32,
            __reserved: 0,
        },
        stx_mtime: StatxTimestamp {
            tv_sec: stat.st_mtime,
            tv_nsec: stat.st_mtime_nsec as u32,
            __reserved: 0,
        },
        stx_rdev_major: linux_major(stat.st_rdev),
        stx_rdev_minor: linux_minor(stat.st_rdev),
        stx_dev_major: linux_major(stat.st_dev),
        stx_dev_minor: linux_minor(stat.st_dev),
        stx_mnt_id: mount_id,
        stx_attributes_mask: STATX_ATTR_MOUNT_ROOT,
        ..Default::default()
    };
    profile::record_hot_syscall_phase(
        HotSyscallPhase::StatxPackOutput,
        profile::scope_start().saturating_sub(pack_output_start),
    );

    let write_user_start = profile::scope_start();
    user_safe::write(statx_ptr, &statx)?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::StatxWriteUser,
        profile::scope_start().saturating_sub(write_user_start),
    );

    Ok(0)
});

define_syscall!(Faccessat, |dirfd: i32,
                            path: CString,
                            mode: i32,
                            flags: AtFlags| {
    let path_str = path_from_raw(path)?;
    faccessat_impl(dirfd, &path_str, mode, flags)
});

define_syscall!(Faccessat2, |dirfd: i32,
                             path: CString,
                             mode: i32,
                             flags: AtFlags| {
    let path_str = path_from_raw(path)?;
    faccessat_impl(dirfd, &path_str, mode, flags)
});
