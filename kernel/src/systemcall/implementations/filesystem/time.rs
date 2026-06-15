use super::*;

define_syscall!(Utimensat, |dirfd: i32,
                            path: u64,
                            times: *const [LinuxTimespec; 2],
                            flags: AtFlags| {
    let allowed_flags = AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH;
    if flags.bits() != (flags & allowed_flags).bits() {
        return Err(SyscallError::InvalidArguments);
    }

    let path = path as CString;
    if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }
        if dirfd >= 0 {
            let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
            let _ = object.as_file_like()?;
        } else {
            return Err(SyscallError::BadAddress);
        }
    } else {
        let path_str = path_from_raw(path)?;
        if path_str.is_empty() {
            if flags.contains(AtFlags::EMPTY_PATH) {
                let object =
                    get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
                let _ = object.as_file_like()?;
            } else {
                return Err(SyscallError::FileNotFound);
            }
        } else {
            let path = resolve_path_at(dirfd, &path_str)?;
            let _ = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
                open_path_nofollow(path)?
            } else {
                open_path(path)?
            };
        }
    }

    if !times.is_null() {
        for timespec in user_safe::read(times)?.iter() {
            if timespec.tv_nsec != UTIME_NOW
                && timespec.tv_nsec != UTIME_OMIT
                && !(0..1_000_000_000).contains(&timespec.tv_nsec)
            {
                return Err(SyscallError::InvalidArguments);
            }
        }
    }

    Ok(0)
});
