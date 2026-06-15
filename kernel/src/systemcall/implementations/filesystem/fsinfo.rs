use super::*;

define_syscall!(Statfs, |path: CString, buf: *mut LinuxStatFs| {
    let path = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path)?;

    let _ = open_path(path.clone())?;
    let statfs = linux_statfs(filesystem_magic_for_path(&path)?);
    user_safe::write(buf, &statfs)?;

    Ok(0)
});

define_syscall!(Fstatfs, |fd: u64, buf: *mut LinuxStatFs| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    let statfs = linux_statfs(filesystem_magic_for_object(&object)?);
    user_safe::write(buf, &statfs)?;
    Ok(0)
});

define_syscall!(Readlink, |path: CString,
                           out_buf: *mut u8,
                           out_len: usize| {
    let path_str = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    readlink_impl(path, out_buf, out_len)
});

define_syscall!(ReadlinkAt, |dirfd: i32,
                             path: CString,
                             out_buf: *mut u8,
                             out_len: usize| {
    let path_str = path_from_raw(path)?;
    if path_str.is_empty() {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        let target = match object.as_file_like()?.read_link() {
            Ok(target) => target,
            Err(FSError::NotASymlink) => return Err(SyscallError::InvalidArguments),
            Err(err) => return Err(err.into()),
        };
        let bytes = target.as_bytes();
        let copied = core::cmp::min(bytes.len(), out_len);
        if copied > 0 {
            user_safe::write(out_buf, &bytes[..copied])?;
        }
        return Ok(copied);
    }
    let path = resolve_path_at(dirfd, &path_str)?;
    readlink_impl(path, out_buf, out_len)
});

define_syscall!(RenameAt, |old_dirfd: i32,
                           old_path: CString,
                           new_dirfd: i32,
                           new_path: CString| {
    let old_path = path_from_raw(old_path)?;
    let new_path = path_from_raw(new_path)?;
    rename_impl(old_dirfd, old_path, new_dirfd, new_path)
});

define_syscall!(RenameAt2, |old_dirfd: i32,
                            old_path: CString,
                            new_dirfd: i32,
                            new_path: CString,
                            flags: u32| {
    let old_path = path_from_raw(old_path)?;
    let new_path = path_from_raw(new_path)?;
    if flags != 0 {
        return Err(SyscallError::NoSyscall);
    }
    rename_impl(old_dirfd, old_path, new_dirfd, new_path)
});
