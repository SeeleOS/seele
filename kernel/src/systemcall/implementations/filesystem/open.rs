use super::*;

define_syscall!(OpenAt, |dirfd: i32,
                         path: CString,
                         flags: OpenFlags,
                         mode: u32| {
    let current_process = get_current_process();
    let path_str = path_from_raw(path)?;
    let create_mode = mode & 0o7777;
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
                    Ok(None) if create => {
                        let create_start = profile::scope_start();
                        create_file_unlocked(path.clone())?;
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
                            Ok(file) => {
                                if create_mode != 0 {
                                    file.chmod(create_mode)?;
                                }
                                Arc::new(file)
                            }
                            Err(err) => return Err(SyscallError::from(err)),
                        }
                    }
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
                create_file_unlocked(path.clone())?;
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
                    Ok(file) => {
                        if create_mode != 0 {
                            file.chmod(create_mode)?;
                        }
                        Arc::new(file)
                    }
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
