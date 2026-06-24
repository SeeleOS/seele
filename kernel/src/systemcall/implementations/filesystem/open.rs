use super::*;

define_syscall!(OpenAt, |dirfd: i32,
                         path: CString,
                         flags: OpenFlags,
                         mode: u32| {
    let current_process = get_current_process();
    let path_str = path_from_raw(path)?;
    let create_mode = {
        let process = current_process.lock();
        mode & 0o7777 & !process.fs_context.lock().file_mode_creation_mask
    };
    if flags.contains(OpenFlags::TMPFILE) {
        let object = open_tmpfile_at(dirfd, &path_str)?;
        if create_mode != 0 {
            let file_like = object.clone().as_file_like()?;
            file_like.chmod(create_mode)?;
        }
        let fd_flags = if flags.contains(OpenFlags::CLOEXEC) {
            FdFlags::CLOEXEC
        } else {
            FdFlags::empty()
        };
        return Ok(current_process
            .lock()
            .push_object_with_flags(object, fd_flags));
    }
    let create = flags.contains(OpenFlags::CREAT);
    let nofollow = flags.contains(OpenFlags::NOFOLLOW);
    let directory_only = flags.contains(OpenFlags::DIRECTORY);
    let path_only = flags.contains(OpenFlags::PATH);

    if create && directory_only {
        return Err(SyscallError::InvalidArguments);
    }

    let resolve_start = profile::scope_start();
    let path = resolve_path_at(dirfd, &path_str)?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::OpenAtPathResolve,
        profile::scope_start().saturating_sub(resolve_start),
    );
    let object = if !nofollow {
        let open_start = profile::scope_start();
        let open_vfs_start = profile::scope_start();
        let open_result = open_path(path.clone());
        profile::record_hot_syscall_phase(
            HotSyscallPhase::OpenAtInitialOpenVfs,
            profile::scope_start().saturating_sub(open_vfs_start),
        );
        profile::record_hot_syscall_phase(
            HotSyscallPhase::OpenAtInitialOpen,
            profile::scope_start().saturating_sub(open_start),
        );
        match open_result {
            Ok(file) => {
                if create && flags.contains(OpenFlags::EXCL) {
                    return Err(SyscallError::FileAlreadyExists);
                }
                Arc::new(file)
            }
            Err(FSError::NotFound) => {
                let proc_self_fd_start = profile::scope_start();
                match proc_self_fd_object(&path) {
                    Ok(Some(object)) => {
                        profile::record_hot_syscall_phase(
                            HotSyscallPhase::OpenAtProcSelfFd,
                            profile::scope_start().saturating_sub(proc_self_fd_start),
                        );
                        object
                    }
                    Ok(None) if create => match open_existing_symlink_target(&path) {
                        Ok(Some(object)) => object,
                        Ok(None) => {
                            let create_start = profile::scope_start();
                            create_file_unlocked(path.clone(), create_mode)?;
                            profile::record_hot_syscall_phase(
                                HotSyscallPhase::OpenAtCreateFile,
                                profile::scope_start().saturating_sub(create_start),
                            );
                            let retry_start = profile::scope_start();
                            let reopen_result = open_path(path.clone());
                            profile::record_hot_syscall_phase(
                                HotSyscallPhase::OpenAtCreateReopen,
                                profile::scope_start().saturating_sub(retry_start),
                            );
                            match reopen_result {
                                Ok(file) => Arc::new(file),
                                Err(err) => return Err(SyscallError::from(err)),
                            }
                        }
                        Err(err) => return Err(err),
                    },
                    Ok(None) => return Err(SyscallError::FileNotFound),
                    Err(err) => return Err(err),
                }
            }
            Err(err) => return Err(SyscallError::from(err)),
        }
    } else {
        let open_start = profile::scope_start();
        let open_vfs_start = profile::scope_start();
        let open_result = open_path_nofollow(path.clone());
        profile::record_hot_syscall_phase(
            HotSyscallPhase::OpenAtInitialOpenVfs,
            profile::scope_start().saturating_sub(open_vfs_start),
        );
        profile::record_hot_syscall_phase(
            HotSyscallPhase::OpenAtInitialOpen,
            profile::scope_start().saturating_sub(open_start),
        );
        match open_result {
            Ok(file) => {
                if create && flags.contains(OpenFlags::EXCL) {
                    return Err(SyscallError::FileAlreadyExists);
                }
                Arc::new(file)
            }
            Err(FSError::NotFound) if create => {
                let create_start = profile::scope_start();
                create_file_unlocked(path.clone(), create_mode)?;
                profile::record_hot_syscall_phase(
                    HotSyscallPhase::OpenAtCreateFile,
                    profile::scope_start().saturating_sub(create_start),
                );
                let retry_start = profile::scope_start();
                let reopen_result = open_path(path.clone());
                profile::record_hot_syscall_phase(
                    HotSyscallPhase::OpenAtCreateReopen,
                    profile::scope_start().saturating_sub(retry_start),
                );
                match reopen_result {
                    Ok(file) => Arc::new(file),
                    Err(err) => return Err(SyscallError::from(err)),
                }
            }
            Err(err) => return Err(SyscallError::from(err)),
        }
    };

    let info_start = profile::scope_start();
    let object_start = profile::scope_start();
    let file_like = object.clone().as_file_like()?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::OpenAtInitialOpenObject,
        profile::scope_start().saturating_sub(object_start),
    );
    let stat_start = profile::scope_start();
    let info = file_like.info()?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::OpenAtInitialOpenStat,
        profile::scope_start().saturating_sub(stat_start),
    );
    profile::record_hot_syscall_phase(
        HotSyscallPhase::OpenAtInfo,
        profile::scope_start().saturating_sub(info_start),
    );
    if nofollow && !path_only && matches!(info.file_like_type, FileLikeType::Symlink) {
        profile::record_hot_syscall_phase(HotSyscallPhase::OpenAtNofollowCheck, 1);
        return Err(SyscallError::TooManySymbolicLinks);
    }
    if nofollow && !path_only {
        profile::record_hot_syscall_phase(HotSyscallPhase::OpenAtNofollowCheck, 1);
    }
    if directory_only && !matches!(info.file_like_type, FileLikeType::Directory) {
        profile::record_hot_syscall_phase(HotSyscallPhase::OpenAtDirectoryCheck, 1);
        return Err(SyscallError::NotADirectory);
    }
    if directory_only {
        profile::record_hot_syscall_phase(HotSyscallPhase::OpenAtDirectoryCheck, 1);
    }
    check_open_permissions(&file_like.stat(), flags)?;
    if flags.contains(OpenFlags::NOATIME) {
        let stat = file_like.stat();
        let credentials = fs_access_credentials();
        if get_current_process().lock().fs_uid != stat.st_uid && !has_capability(&credentials, 3) {
            return Err(SyscallError::PermissionDenied);
        }
    }
    ensure_open_writable_mount(&path, flags)?;
    if flags.contains(OpenFlags::TRUNC) && !path_only {
        let truncate_start = profile::scope_start();
        let file_like = object.clone().as_file_like()?;
        if file_like.is_device_backed() {
            // Linux ignores O_TRUNC on device nodes such as /dev/null.
            // Only regular writable files should be truncated here.
        } else {
            match info.file_like_type {
                FileLikeType::File => file_like.truncate(0)?,
                FileLikeType::Directory => return Err(SyscallError::IsADirectory),
                FileLikeType::Symlink => {}
            }
        }
        profile::record_hot_syscall_phase(
            HotSyscallPhase::OpenAtTruncate,
            profile::scope_start().saturating_sub(truncate_start),
        );
    }
    let mut file_flags = match flags.bits() & 0o3 {
        0o1 => FileFlags::WRONLY,
        0o2 => FileFlags::RDWR,
        _ => FileFlags::empty(),
    };
    if flags.contains(OpenFlags::APPEND) {
        file_flags.insert(FileFlags::APPEND);
    }
    if flags.contains(OpenFlags::NONBLOCK) {
        file_flags.insert(FileFlags::NONBLOCK);
    }
    if !file_flags.is_empty() {
        let set_flags_start = profile::scope_start();
        match object.clone().set_flags(file_flags) {
            Ok(()) | Err(ObjectError::Unimplemented) => {}
            Err(err) => return Err(err.into()),
        }
        profile::record_hot_syscall_phase(
            HotSyscallPhase::OpenAtSetFlags,
            profile::scope_start().saturating_sub(set_flags_start),
        );
    }

    let fd_flags = if flags.contains(OpenFlags::CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let install_fd_start = profile::scope_start();
    let fd = current_process
        .lock()
        .push_object_with_flags(object, fd_flags);
    profile::record_hot_syscall_phase(
        HotSyscallPhase::OpenAtInstallFd,
        profile::scope_start().saturating_sub(install_fd_start),
    );
    Ok(fd)
});

define_syscall!(Open, |path: CString, flags: OpenFlags, mode: u32| {
    OpenAt::handle_call(
        (-100i32) as u64,
        path as u64,
        flags.bits() as u64,
        mode as u64,
        0,
        0,
    )
});

define_syscall!(OpenAt2, |dirfd: i32,
                          path: CString,
                          how_ptr: *const LinuxOpenHow,
                          size: usize| {
    let (flags, mode, resolve) = read_open_how(how_ptr, size)?;
    let path_str = path_from_raw(path)?;
    validate_openat2_resolve(dirfd, &path_str, resolve)?;
    OpenAt::handle_call(
        dirfd as u64,
        path as u64,
        flags.bits() as u64,
        mode as u64,
        0,
        0,
    )
});

fn read_open_how(
    how_ptr: *const LinuxOpenHow,
    size: usize,
) -> Result<(OpenFlags, u32, OpenResolveFlags), SyscallError> {
    let base_size = size_of::<LinuxOpenHow>();
    if how_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }
    if size < base_size {
        return Err(SyscallError::InvalidArguments);
    }

    let how = user_safe::read(how_ptr).map_err(|_| SyscallError::BadAddress)?;
    if size > base_size {
        let extra = size - base_size;
        let tail_ptr = unsafe { (how_ptr as *const u8).add(base_size) };
        let tail = user_safe::read_buffer(tail_ptr, extra).map_err(|_| SyscallError::BadAddress)?;
        if tail.iter().any(|byte| *byte != 0) {
            return Err(SyscallError::ArgumentListTooLong);
        }
    }

    if how.flags > i32::MAX as u64 {
        return Err(SyscallError::InvalidArguments);
    }
    let flags_bits = how.flags as i32;
    let known_flag_bits = OpenFlags::all().bits() | 0o3;
    if flags_bits & !known_flag_bits != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let flags = OpenFlags::from_bits_retain(flags_bits);
    let resolve = OpenResolveFlags::from_bits(how.resolve).ok_or(SyscallError::InvalidArguments)?;
    if how.mode & !0o7777 != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if how.mode != 0 && !(flags.contains(OpenFlags::CREAT) || flags.contains(OpenFlags::TMPFILE)) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok((flags, how.mode as u32, resolve))
}

fn open_existing_symlink_target(path: &Path) -> Result<Option<ObjectRef>, SyscallError> {
    let nofollow = match open_path_nofollow(path.clone()) {
        Ok(object) => object,
        Err(FSError::NotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    if !matches!(nofollow.info()?.file_like_type, FileLikeType::Symlink) {
        return Ok(None);
    }
    let target = Path::new(&nofollow.read_link()?);
    let target = if target.is_absolute() {
        target
    } else {
        let mut combined = path.parent().unwrap_or_default().as_string();
        if !combined.ends_with('/') {
            combined.push('/');
        }
        combined.push_str(&target.as_string());
        Path::new(&combined).normalize()
    };
    Ok(Some(Arc::new(open_path(target)?)))
}

fn validate_openat2_resolve(
    dirfd: i32,
    path: &str,
    resolve: OpenResolveFlags,
) -> Result<(), SyscallError> {
    if resolve.is_empty() {
        return Ok(());
    }

    if resolve.contains(OpenResolveFlags::RESOLVE_BENEATH)
        && (path.starts_with('/') || path.split('/').any(|part| part == ".."))
    {
        return Err(SyscallError::CrossDeviceLink);
    }
    if resolve.contains(OpenResolveFlags::RESOLVE_IN_ROOT) && path.starts_with('/') {
        return Err(SyscallError::FileNotFound);
    }
    if resolve.contains(OpenResolveFlags::RESOLVE_NO_XDEV) && path.starts_with("/proc/") {
        return Err(SyscallError::CrossDeviceLink);
    }
    if resolve.contains(OpenResolveFlags::RESOLVE_NO_MAGICLINKS) && path == "/proc/self/exe" {
        return Err(SyscallError::TooManySymbolicLinks);
    }
    if resolve.contains(OpenResolveFlags::RESOLVE_NO_SYMLINKS) {
        let resolved = resolve_path_at(dirfd, path)?;
        if matches!(
            open_path_nofollow(resolved)?.info()?.file_like_type,
            FileLikeType::Symlink
        ) {
            return Err(SyscallError::TooManySymbolicLinks);
        }
    }

    Ok(())
}

fn ensure_open_writable_mount(path: &Path, flags: OpenFlags) -> Result<(), SyscallError> {
    if flags.contains(OpenFlags::PATH) {
        return Ok(());
    }

    let access_mode = flags.bits() & 0o3;
    let wants_write = access_mode == 0o1 || access_mode == 0o2 || flags.contains(OpenFlags::TRUNC);
    if !wants_write {
        return Ok(());
    }

    match VirtualFS.lock().ensure_writable_mount(path.clone()) {
        Ok(()) => Ok(()),
        Err(FSError::Readonly) if is_proc_sysctl_path(path) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn is_proc_sysctl_path(path: &Path) -> bool {
    let path = path.clone().as_string();
    path.starts_with("/proc/sys/") || path.starts_with("/sys/")
}

define_syscall!(Creat, |path: CString, mode: u32| {
    OpenAt::handle_call(
        (-100i32) as u64,
        path as u64,
        (OpenFlags::CREAT | OpenFlags::TRUNC).bits() as u64 | 0o1,
        mode as u64,
        0,
        0,
    )
});
