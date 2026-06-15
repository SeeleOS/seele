use super::*;

pub(in crate::systemcall::implementations::filesystem) fn check_access_mode(
    mode: i32,
) -> Result<(), SyscallError> {
    if (mode & !7) != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(())
}

pub(in crate::systemcall::implementations::filesystem) fn check_access_permissions(
    stat: &LinuxStat,
    mode: i32,
) -> Result<(), SyscallError> {
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

pub(in crate::systemcall::implementations::filesystem) fn faccessat_impl(
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

pub(in crate::systemcall::implementations::filesystem) fn chmod_at(
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

pub(in crate::systemcall::implementations::filesystem) fn chmod_fd_object(
    object: ObjectRef,
    mode: u32,
) -> Result<(), SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        file_like.chmod(mode)?;
    } else {
        let _ = object.as_statable()?;
    }

    Ok(())
}

pub(in crate::systemcall::implementations::filesystem) fn chown_at(
    dirfd: i32,
    path_str: &str,
    flags: AtFlags,
) -> Result<usize, SyscallError> {
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

pub(in crate::systemcall::implementations::filesystem) fn chown_fd_object(
    object: ObjectRef,
) -> Result<(), SyscallError> {
    if object.clone().as_file_like().is_err() {
        let _ = object.as_statable()?;
    }

    Ok(())
}
