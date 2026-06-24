use super::*;

const CAP_DAC_OVERRIDE: u64 = 1;
const CAP_DAC_READ_SEARCH: u64 = 2;

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
    check_access_permissions_for_ids(stat, mode, &access_credentials(false))
}

#[derive(Clone)]
pub(in crate::systemcall::implementations) struct AccessCredentials {
    uid: u32,
    gid: u32,
    supplementary_groups: Vec<u32>,
    capability_effective: [u32; 2],
}

fn access_credentials(use_effective_ids: bool) -> AccessCredentials {
    let process = get_current_process();
    let process = process.lock();
    AccessCredentials {
        uid: if use_effective_ids {
            process.effective_uid
        } else {
            process.real_uid
        },
        gid: if use_effective_ids {
            process.effective_gid
        } else {
            process.real_gid
        },
        supplementary_groups: process.supplementary_groups.clone(),
        capability_effective: if use_effective_ids {
            process.capability_effective
        } else {
            [0; 2]
        },
    }
}

pub(in crate::systemcall::implementations) fn fs_access_credentials() -> AccessCredentials {
    let process = get_current_process();
    let process = process.lock();
    AccessCredentials {
        uid: process.fs_uid,
        gid: process.fs_gid,
        supplementary_groups: process.supplementary_groups.clone(),
        capability_effective: process.capability_effective,
    }
}

pub(in crate::systemcall::implementations::filesystem) fn check_open_permissions(
    stat: &LinuxStat,
    flags: OpenFlags,
) -> Result<(), SyscallError> {
    if flags.contains(OpenFlags::PATH) {
        return Ok(());
    }

    let mode = match flags.bits() & 0o3 {
        0o0 => 4,
        0o1 => 2,
        0o2 => 4 | 2,
        _ => return Err(SyscallError::InvalidArguments),
    };

    check_access_permissions_for_ids(stat, mode, &fs_access_credentials())
}

fn check_access_permissions_for_ids(
    stat: &LinuxStat,
    mode: i32,
    credentials: &AccessCredentials,
) -> Result<(), SyscallError> {
    check_access_permissions_for_ids_with_options(stat, mode, credentials, false)
}

fn has_capability(credentials: &AccessCredentials, capability: u64) -> bool {
    let slot = (capability / 32) as usize;
    let mask = 1u32 << (capability % 32);
    credentials
        .capability_effective
        .get(slot)
        .is_some_and(|value| value & mask != 0)
}

pub(in crate::systemcall::implementations) fn check_access_permissions_for_ids_with_options(
    stat: &LinuxStat,
    mode: i32,
    credentials: &AccessCredentials,
    allow_root_directory_search: bool,
) -> Result<(), SyscallError> {
    let permission = stat.st_mode & 0o777;
    if credentials.uid == 0 {
        if allow_root_directory_search && (stat.st_mode & S_IFMT) == S_IFDIR {
            return Ok(());
        }
        if (mode & 1) != 0 && permission & 0o111 == 0 {
            return Err(SyscallError::AccessDenied);
        }
        return Ok(());
    }

    if mode & 1 == 0 {
        if mode & 4 != 0 && has_capability(credentials, CAP_DAC_READ_SEARCH) {
            return Ok(());
        }
        if mode & 2 != 0 && has_capability(credentials, CAP_DAC_OVERRIDE) {
            return Ok(());
        }
    }

    let permission_shift = if credentials.uid == stat.st_uid {
        6
    } else if credentials.gid == stat.st_gid
        || credentials.supplementary_groups.contains(&stat.st_gid)
    {
        3
    } else {
        0
    };
    let permission = (permission >> permission_shift) & 0o7;

    if (mode & 4) != 0 && permission & 0o4 == 0 {
        return Err(SyscallError::AccessDenied);
    }
    if (mode & 2) != 0 && permission & 0o2 == 0 {
        return Err(SyscallError::AccessDenied);
    }
    if (mode & 1) != 0 && permission & 0o1 == 0 {
        return Err(SyscallError::AccessDenied);
    }

    Ok(())
}

fn check_effective_access_permissions(stat: &LinuxStat, mode: i32) -> Result<(), SyscallError> {
    check_access_permissions_for_ids(stat, mode, &access_credentials(true))
}

pub(in crate::systemcall::implementations) fn check_access_path_search_permissions(
    path: &Path,
    credentials: &AccessCredentials,
) -> Result<(), SyscallError> {
    if credentials.uid == 0 {
        return Ok(());
    }

    let path = path.normalize();
    let normal_parts = path
        .parts
        .iter()
        .filter(|part| matches!(part, crate::filesystem::path::PathPart::Normal(_)))
        .count();
    if normal_parts == 0 {
        return Ok(());
    }

    let mut prefix = Path::default();
    check_access_permissions_for_ids(&open_path(prefix.clone())?.stat(), 1, credentials)?;

    let mut seen = 0;
    for part in path.parts {
        let crate::filesystem::path::PathPart::Normal(component) = part else {
            continue;
        };

        seen += 1;
        if seen == normal_parts {
            break;
        }

        prefix = prefix.join_component(&component);
        let directory = open_path(prefix.clone())?;
        if !matches!(directory.info()?.file_like_type, FileLikeType::Directory) {
            return Err(SyscallError::NotADirectory);
        }
        check_access_permissions_for_ids(&directory.stat(), 1, credentials)?;
    }

    Ok(())
}

pub(in crate::systemcall::implementations::filesystem) fn check_access_target(
    path: Path,
    mode: i32,
    flags: AtFlags,
) -> Result<(), SyscallError> {
    let credentials = access_credentials(flags.contains(AtFlags::EACCESS));
    check_access_path_search_permissions(&path, &credentials)?;
    let object = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path)
    } else {
        open_path(path)
    }?;
    check_access_permissions_for_ids(&object.stat(), mode, &credentials)
}

pub(in crate::systemcall::implementations::filesystem) fn check_chdir_target(
    path: &Path,
    stat: &LinuxStat,
) -> Result<(), SyscallError> {
    let credentials = fs_access_credentials();
    check_access_path_search_permissions(path, &credentials)?;
    check_access_permissions_for_ids_with_options(stat, 1, &credentials, true)
}

pub(in crate::systemcall::implementations::filesystem) fn faccessat_impl(
    dirfd: i32,
    path_str: &str,
    mode: i32,
    flags: AtFlags,
) -> Result<usize, SyscallError> {
    let allowed = (AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW | AtFlags::EACCESS).bits();
    if flags.bits() != flags.bits() & allowed {
        return Err(SyscallError::InvalidArguments);
    }

    check_access_mode(mode)?;

    if path_str.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }

        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        let stat = object.as_statable()?.stat();
        if flags.contains(AtFlags::EACCESS) {
            check_effective_access_permissions(&stat, mode)?;
        } else {
            check_access_permissions(&stat, mode)?;
        }
        return Ok(0);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    check_access_target(path, mode, flags)?;
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
    uid: u32,
    gid: u32,
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
        chown_fd_object(
            get_object_current_process(dirfd as u64).map_err(SyscallError::from)?,
            uid,
            gid,
        )?;
        return Ok(0);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let file = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path)?
    } else {
        open_path(path)?
    };
    if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        file.lchown(uid, gid)?;
    } else {
        file.chown(uid, gid)?;
    }
    Ok(0)
}

pub(in crate::systemcall::implementations::filesystem) fn chown_fd_object(
    object: ObjectRef,
    uid: u32,
    gid: u32,
) -> Result<(), SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        file_like.chown(uid, gid)?;
    } else {
        let _ = object.as_statable()?;
    }

    Ok(())
}
