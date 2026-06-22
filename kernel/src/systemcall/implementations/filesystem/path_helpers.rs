use super::*;

pub(super) fn path_from_raw(path: CString) -> Result<String, SyscallError> {
    const PATH_MAX: usize = 4096;

    if path.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut out = String::new();
    for index in 0..=PATH_MAX {
        let byte =
            user_safe::read(unsafe { path.add(index) }).map_err(|_| SyscallError::BadAddress)?;
        if byte == 0 {
            return Ok(out);
        }
        if index == PATH_MAX {
            return Err(SyscallError::PathTooLong);
        }
        out.push(byte as char);
    }

    unreachable!()
}

pub(super) fn string_from_raw_optional(value: CString) -> Result<Option<String>, SyscallError> {
    if value.is_null() {
        return Ok(None);
    }

    String::k_from(value)
        .map(Some)
        .map_err(|_| SyscallError::InvalidArguments)
}
pub(super) fn resolve_path_at(dirfd: i32, path_str: &str) -> Result<Path, SyscallError> {
    if path_str.is_empty() {
        return Err(SyscallError::FileNotFound);
    }

    let path = Path::new(path_str);
    let process = get_current_process();
    let fs_context = process.lock().fs_context.lock().clone();

    if path.is_absolute() {
        return Ok(AbsolutePath::join_under_root(
            &fs_context.root_directory,
            &fs_context.current_directory,
            &path,
        )
        .as_normal());
    }

    if dirfd == AT_FDCWD {
        let mut current_dir = fs_context.current_directory;
        current_dir.push_path_str(path_str);
        return Ok(current_dir.as_normal());
    }

    let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
    let file_like = object
        .as_file_like()
        .map_err(|_| SyscallError::NotADirectory)?;
    if !matches!(file_like.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }

    let base_path = file_like.path();
    let base = AbsolutePath::from_root_path(&base_path);
    let mut base = AbsolutePath::join_under_root(&base, &base, &Path::new("."));
    base.push_path_str(path_str);
    Ok(base.as_normal())
}

pub(super) fn next_tmpfile_path(dir_path: &Path) -> Path {
    let dir_path = dir_path.clone().normalize().as_string();
    let tmp_id = NEXT_TMPFILE_ID.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(".tmpfile-{tmp_id}");
    let path = if dir_path == "/" {
        format!("/{tmp_name}")
    } else {
        format!("{dir_path}/{tmp_name}")
    };
    Path::new(&path)
}

pub(super) fn open_tmpfile_at(dirfd: i32, path_str: &str) -> Result<ObjectRef, SyscallError> {
    let dir_path = resolve_path_at(dirfd, path_str)?;
    let dir = open_path(dir_path.clone())?;
    if !matches!(dir.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }

    for _ in 0..128 {
        let tmp_path = next_tmpfile_path(&dir_path);
        let create_result = VirtualFS.lock().create_file(tmp_path.clone());
        match create_result {
            Ok(()) => {
                let object: ObjectRef = Arc::new(open_path(tmp_path.clone())?);
                VirtualFS.lock().delete_file(tmp_path)?;
                return Ok(object);
            }
            Err(FSError::AlreadyExists) => continue,
            Err(err) => return Err(SyscallError::from(err)),
        }
    }

    Err(SyscallError::FileAlreadyExists)
}

pub(super) fn proc_self_fd_object(path: &Path) -> Result<Option<ObjectRef>, SyscallError> {
    let path = path.clone().normalize().as_string();
    let Some(fd_str) = path.strip_prefix("/proc/self/fd/") else {
        return Ok(None);
    };
    if fd_str.is_empty() || fd_str.contains('/') {
        return Ok(None);
    }

    let fd = fd_str
        .parse::<u64>()
        .map_err(|_| SyscallError::FileNotFound)?;
    let object = match get_object_current_process(fd) {
        Ok(object) => object,
        Err(_) => return Err(SyscallError::FileNotFound),
    };
    Ok(Some(object))
}
pub(super) fn create_file_unlocked(path: Path) -> Result<(), SyscallError> {
    let (parent_dir, name) = {
        let vfs = VirtualFS.lock();
        let normalized = vfs.normalize_path(path.clone());
        if normalized.ends_with_slash() {
            return Err(SyscallError::NotADirectory);
        }
        vfs.resolve_parent(path).map_err(SyscallError::from)?
    };

    parent_dir
        .lock()
        .create(DirectoryContentInfo::new(name, DirectoryContentType::File))
        .map_err(SyscallError::from)
}

pub(super) fn profile_mkdir_common(dirfd: i32, path: &str, mode: u32) -> Result<(), SyscallError> {
    let resolve_start = profile::scope_start();
    let path = resolve_path_at(dirfd, path)?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::MkdirPathResolve,
        profile::scope_start().saturating_sub(resolve_start),
    );

    let create_start = profile::scope_start();
    VirtualFS.lock().create_dir_with_mode(path, Some(mode))?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::MkdirCreateDir,
        profile::scope_start().saturating_sub(create_start),
    );

    let apply_mode_start = profile::scope_start();
    let _ = mode;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::MkdirApplyMode,
        profile::scope_start().saturating_sub(apply_mode_start),
    );
    Ok(())
}
pub(super) fn readlink_impl(
    path: Path,
    out_buf: *mut u8,
    out_len: usize,
) -> Result<usize, SyscallError> {
    let target = match open_path_nofollow(path)?.read_link() {
        Ok(target) => target,
        Err(FSError::NotASymlink) => return Err(SyscallError::InvalidArguments),
        Err(err) => return Err(err.into()),
    };
    let bytes = target.as_bytes();
    let copied = core::cmp::min(bytes.len(), out_len);
    if copied > 0 {
        user_safe::write(out_buf, &bytes[..copied])?;
    }

    Ok(copied)
}
pub(super) fn rename_impl(
    old_dirfd: i32,
    old_path: String,
    new_dirfd: i32,
    new_path: String,
) -> Result<usize, SyscallError> {
    let old_path = resolve_path_at(old_dirfd, &old_path)?;
    let new_path = resolve_path_at(new_dirfd, &new_path)?;

    if old_path.clone().as_string() == new_path.clone().as_string() {
        return Ok(0);
    }

    let _ = open_path_nofollow(old_path.clone())?;
    if let Some(parent) = new_path.parent() {
        let _ = open_path(parent)?;
    }

    VirtualFS
        .lock()
        .rename_file(old_path.clone(), new_path.clone())
        .map_err(SyscallError::from)?;
    Ok(0)
}
