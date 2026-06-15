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

    let old_path = path_from_raw(old_path)?;
    if old_path.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }
        let object = get_object_current_process(old_dirfd as u64).map_err(SyscallError::from)?;
        let result = object.as_file_like()?.link_to(new_path.clone());
        let result = result.map_err(SyscallError::from);
        result?;
        return Ok(0);
    }

    let old_path = resolve_path_at(old_dirfd, &old_path)?;
    VirtualFS.lock().link_file(old_path, new_path)?;

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
    profile_mkdir_common(dirfd, &path, mode)?;
    Ok(0)
});

define_syscall!(Mknodat, |dirfd: i32,
                          path: CString,
                          mode: u32,
                          _dev: u64| {
    let path = path_from_raw(path)?;
    let path = resolve_path_at(dirfd, &path)?;

    match mode & S_IFMT {
        0 | S_IFREG | S_IFIFO | S_IFCHR | S_IFBLK | S_IFSOCK => {
            VirtualFS.lock().create_file(path.clone())?;
            let file = open_path(path)?;
            file.chmod(mode)?;
            Ok(0)
        }
        _ => Err(SyscallError::NoSyscall),
    }
});

define_syscall!(Mkdir, |path: CString, mode: u32| {
    let path = path_from_raw(path)?;
    let mode = mode & !S_IFMT;
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
