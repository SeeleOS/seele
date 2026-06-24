use super::*;

define_syscall!(UnlinkAt, |dirfd: i32, path: CString, flags: AtFlags| {
    let path = path_from_raw(path)?;
    if flags.bits() & !AtFlags::REMOVEDIR.bits() != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let path = resolve_path_at(dirfd, &path)?;
    let object = open_path_nofollow(path.clone())?;
    let is_directory = matches!(object.info()?.file_like_type, FileLikeType::Directory);
    if flags.contains(AtFlags::REMOVEDIR) {
        if !is_directory {
            return Err(SyscallError::NotADirectory);
        }
    } else if is_directory {
        return Err(SyscallError::IsADirectory);
    }
    let result = VirtualFS.lock().delete_file(path.clone());
    let result = result.map_err(SyscallError::from);
    result?;
    Ok(0)
});

define_syscall!(LinkAt, |old_dirfd: i32,
                         old_path: CString,
                         new_dirfd: i32,
                         new_path: CString,
                         flags: AtFlags| {
    let allowed_flags = AtFlags::EMPTY_PATH | AtFlags::SYMLINK_FOLLOW;
    if flags.bits() != (flags & allowed_flags).bits() {
        return Err(SyscallError::InvalidArguments);
    }
    let new_path = path_from_raw(new_path)?;
    let new_path = resolve_path_at(new_dirfd, &new_path)?;

    if old_path.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let old_path_str = path_from_raw(old_path)?;
    if old_path_str.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }
        let object = get_object_current_process(old_dirfd as u64).map_err(SyscallError::from)?;
        let file_like = object.as_file_like()?;
        if matches!(file_like.info()?.file_like_type, FileLikeType::Directory) {
            return Err(SyscallError::PermissionDenied);
        }
        let result = file_like.link_to(new_path.clone());
        let result = result.map_err(|err| match err {
            FSError::Other => SyscallError::CrossDeviceLink,
            err => SyscallError::from(err),
        });
        result?;
        return Ok(0);
    }

    let old_path_is_relative = !Path::new(&old_path_str).is_absolute();
    let old_path = resolve_path_at(old_dirfd, &old_path_str)?;
    if matches!(
        open_path_nofollow(old_path.clone())?.info()?.file_like_type,
        FileLikeType::Directory
    ) {
        return Err(SyscallError::PermissionDenied);
    }
    if old_dirfd != AT_FDCWD && old_path_is_relative {
        let object = get_object_current_process(old_dirfd as u64).map_err(SyscallError::from)?;
        let file_like = object
            .as_file_like()
            .map_err(|_| SyscallError::NotADirectory)?;
        if !matches!(file_like.info()?.file_like_type, FileLikeType::Directory) {
            return Err(SyscallError::NotADirectory);
        }
        open_path(file_like.path()).map_err(|_| SyscallError::FileNotFound)?;
    }
    VirtualFS
        .lock()
        .link_file(old_path, new_path)
        .map_err(|err| match err {
            FSError::Other => SyscallError::CrossDeviceLink,
            err => SyscallError::from(err),
        })?;

    Ok(0)
});

define_syscall!(SymlinkAt, |target: CString,
                            new_dirfd: i32,
                            link_path: CString| {
    let target = path_from_raw(target)?;
    let link_path = path_from_raw(link_path)?;
    let link_path = resolve_path_at(new_dirfd, &link_path)?;

    VirtualFS.lock().create_symlink(link_path, &target)?;

    Ok(0)
});

define_syscall!(MkdirAt, |dirfd: i32, path: CString, mode: u32| {
    let path = path_from_raw(path)?;
    let mode = mode & !S_IFMT;
    let resolved = resolve_path_at(dirfd, &path)?;
    if let Some(parent) = resolved.parent() {
        check_access_path_search_permissions(&parent, &fs_access_credentials())?;
        check_access_permissions_for_ids_with_options(
            &open_path(parent)?.stat(),
            3,
            &fs_access_credentials(),
            false,
        )?;
    }
    profile_mkdir_common(dirfd, &path, mode)?;
    Ok(0)
});

define_syscall!(Mknodat, |dirfd: i32, path: CString, mode: u32, dev: u64| {
    let path = path_from_raw(path)?;
    let path = resolve_path_at(dirfd, &path)?;
    let umask = {
        let process = get_current_process();
        let process = process.lock();
        process.fs_context.lock().file_mode_creation_mask
    };
    let file_type = mode & S_IFMT;
    let create_mode = file_type | (mode & 0o7777 & !umask);

    match file_type {
        0 | S_IFREG | S_IFIFO | S_IFCHR | S_IFBLK | S_IFSOCK => {
            VirtualFS
                .lock()
                .create_file_with_metadata(path.clone(), Some(create_mode), dev)?;
            Ok(0)
        }
        _ => Err(SyscallError::NoSyscall),
    }
});

define_syscall!(Mkdir, |path: CString, mode: u32| {
    let path = path_from_raw(path)?;
    let mode = mode & !S_IFMT;
    let resolved = resolve_path_at(AT_FDCWD, &path)?;
    if let Some(parent) = resolved.parent() {
        check_access_path_search_permissions(&parent, &fs_access_credentials())?;
        check_access_permissions_for_ids_with_options(
            &open_path(parent)?.stat(),
            3,
            &fs_access_credentials(),
            false,
        )?;
    }
    profile_mkdir_common(AT_FDCWD, &path, mode)?;
    Ok(0)
});

define_syscall!(Rmdir, |path: CString| {
    let path = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path)?;

    let object = open_path_nofollow(path.clone())?;
    let is_directory = matches!(object.info()?.file_like_type, FileLikeType::Directory);
    if !is_directory {
        return Err(SyscallError::NotADirectory);
    }

    VirtualFS.lock().delete_file(path)?;
    Ok(0)
});
