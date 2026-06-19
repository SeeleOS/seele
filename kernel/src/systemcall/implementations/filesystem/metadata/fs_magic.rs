use super::*;

pub(in crate::systemcall::implementations::filesystem) fn filesystem_magic_for_file_like(
    file_like: &FileLikeObject,
) -> Result<i64, SyscallError> {
    filesystem_magic_for_path(&file_like.path())
}

pub(in crate::systemcall::implementations::filesystem) fn filesystem_magic_for_object(
    object: &ObjectRef,
) -> Result<i64, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return filesystem_magic_for_file_like(&file_like);
    }

    if object.clone().as_pidfd().is_ok()
        || object.clone().as_eventfd().is_ok()
        || object.clone().as_inotify().is_ok()
        || object.clone().as_poller().is_ok()
        || object.clone().as_signalfd().is_ok()
        || object.clone().as_timerfd().is_ok()
    {
        return Ok(ANON_INODE_FS_MAGIC);
    }

    if object.clone().as_inet_socket().is_ok()
        || object.clone().as_netlink_socket().is_ok()
        || object.clone().as_unix_socket().is_ok()
    {
        return Ok(SOCKFS_MAGIC);
    }

    if object.clone().as_pipe().is_ok() {
        return Ok(PIPEFS_MAGIC);
    }

    Err(SyscallError::BadFileDescriptor)
}

pub(in crate::systemcall::implementations::filesystem) fn filesystem_magic_for_path(
    path: &Path,
) -> Result<i64, SyscallError> {
    let fs = {
        let (_mount_path, fs, _, _) = VirtualFS.lock().mount_metadata(path.clone())?;
        fs
    };
    Ok(fs.lock().magic())
}

pub(in crate::systemcall::implementations::filesystem) fn mount_id_for_file_like(
    file_like: &FileLikeObject,
) -> Result<u64, SyscallError> {
    Ok(file_like.mount_id())
}

pub(in crate::systemcall::implementations::filesystem) fn pseudo_mount_id(
    magic: i64,
) -> Option<u64> {
    let offset = match magic {
        SOCKFS_MAGIC => 0,
        ANON_INODE_FS_MAGIC => 1,
        PIPEFS_MAGIC => 2,
        _ => return None,
    };

    let mount_count = VirtualFS.lock().mount_count() as u64;
    Some(mount_count + 1 + offset)
}

pub(in crate::systemcall::implementations::filesystem) fn mount_id_for_object(
    object: &ObjectRef,
) -> Result<u64, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return mount_id_for_file_like(&file_like);
    }

    let magic = filesystem_magic_for_object(object)?;
    pseudo_mount_id(magic).ok_or(SyscallError::BadFileDescriptor)
}

pub(in crate::systemcall::implementations::filesystem) fn mount_root_for_object(
    object: &ObjectRef,
) -> Result<bool, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return Ok(file_like.mount_root());
    }

    let magic = filesystem_magic_for_object(object)?;
    if pseudo_mount_id(magic).is_some() {
        return Ok(false);
    }

    Err(SyscallError::BadFileDescriptor)
}

pub(in crate::systemcall::implementations::filesystem) fn stat_mount_id_at(
    dirfd: i32,
    path_str: &str,
    flags: AtFlags,
) -> Result<u64, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return mount_id_for_object(&object);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    Ok(VirtualFS.lock().mount_id(path)?)
}
