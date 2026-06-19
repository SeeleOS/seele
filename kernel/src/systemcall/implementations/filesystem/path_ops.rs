use super::*;

define_syscall!(Access, |path: CString, mode: i32| {
    check_access_mode(mode)?;
    let path_str = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    check_access_target(path, mode, AtFlags::empty())?;
    Ok(0)
});

define_syscall!(Chdir, |dir: String| {
    let process = get_current_process();
    let fs_context = process.lock().fs_context.lock().clone();
    let path =
        Path::new(&dir).as_absolute_from(&fs_context.root_directory, &fs_context.current_directory);
    let object = open_path(path.as_normal())?;
    if !matches!(object.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }
    check_chdir_target(&path.as_normal(), &object.stat())?;
    get_current_process().lock().change_directory(path)?;
    Ok(0)
});

define_syscall!(Fchdir, |fd: u64| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    let file_like = object.as_file_like()?;
    if !matches!(file_like.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }
    let path = AbsolutePath::from_root_path(&file_like.path());
    get_current_process().lock().change_directory(path)?;
    Ok(0)
});

define_syscall!(Link, |old_path: CString, new_path: CString| {
    LinkAt::handle_call(
        AT_FDCWD as u64,
        old_path as u64,
        AT_FDCWD as u64,
        new_path as u64,
        0,
        0,
    )
});

define_syscall!(Rename, |old_path: CString, new_path: CString| {
    RenameAt::handle_call(
        AT_FDCWD as u64,
        old_path as u64,
        AT_FDCWD as u64,
        new_path as u64,
        0,
        0,
    )
});

define_syscall!(Unlink, |path: CString| {
    UnlinkAt::handle_call(AT_FDCWD as u64, path as u64, 0, 0, 0, 0)
});

define_syscall!(Symlink, |target: CString, link_path: CString| {
    let target = path_from_raw(target)?;
    let link_path = path_from_raw(link_path)?;
    let link_path = resolve_path_at(AT_FDCWD, &link_path)?;

    VirtualFS.lock().create_symlink(link_path, &target)?;
    Ok(0)
});

define_syscall!(Chmod, |path: CString, mode: u32| {
    let path_str = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    let file = open_path(path)?;
    file.chmod(mode)?;
    Ok(0)
});

define_syscall!(Chown, |path: CString, owner: u32, group: u32| {
    let path_str = path_from_raw(path)?;
    chown_at(AT_FDCWD, &path_str, owner, group, AtFlags::empty())
});

define_syscall!(Getcwd, |buf_ptr: *mut u8, len: usize| {
    let process = get_current_process();
    let fs_context = process.lock().fs_context.lock().clone();
    let path_str = fs_context
        .current_directory
        .display_string(&fs_context.root_directory);
    let path_bytes = path_str.as_bytes();
    let path_len = path_bytes.len();

    if len <= path_len {
        return Err(SyscallError::RangeError);
    }
    if buf_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut buffer = Vec::with_capacity(path_len + 1);
    buffer.extend_from_slice(path_bytes);
    buffer.push(0);
    user_safe::write(buf_ptr, &buffer[..])?;

    Ok(path_len + 1)
});

define_syscall!(Chroot, |path: CString| {
    let path_str = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    let file = open_path(path.clone())?;
    if !matches!(file.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }

    let new_root = AbsolutePath::from_root_path(&path);
    let process = get_current_process();
    let process = &mut *process.lock();
    let current_dir = process.fs_context.lock().current_directory.clone();
    if !current_dir.starts_with(&new_root) {
        process.fs_context.lock().current_directory = AbsolutePath::root();
    }
    process.fs_context.lock().root_directory = new_root;
    Ok(0)
});
