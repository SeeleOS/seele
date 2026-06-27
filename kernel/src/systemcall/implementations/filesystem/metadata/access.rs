use super::*;

const CAP_DAC_OVERRIDE: u64 = 1;
const CAP_DAC_READ_SEARCH: u64 = 2;
const CAP_CHOWN: u64 = 0;
const CAP_FOWNER: u64 = 3;
const CAP_FSETID: u64 = 4;
const S_ISUID: u32 = 0o4000;
const S_ISGID: u32 = 0o2000;
const S_IXGRP: u32 = 0o010;
const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;

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

pub(in crate::systemcall::implementations::filesystem) fn fs_gid() -> u32 {
    fs_access_credentials().gid
}

pub(in crate::systemcall::implementations::filesystem) fn fs_uid() -> u32 {
    fs_access_credentials().uid
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

pub(in crate::systemcall::implementations) fn has_capability(
    credentials: &AccessCredentials,
    capability: u64,
) -> bool {
    let slot = (capability / 32) as usize;
    let mask = 1u32 << (capability % 32);
    credentials
        .capability_effective
        .get(slot)
        .is_some_and(|value| value & mask != 0)
}

fn is_group_member(credentials: &AccessCredentials, gid: u32) -> bool {
    credentials.gid == gid || credentials.supplementary_groups.contains(&gid)
}

pub(in crate::systemcall::implementations::filesystem) fn strip_sgid_for_create(
    mode: u32,
    gid: u32,
) -> u32 {
    let credentials = fs_access_credentials();
    if (mode & S_ISGID) != 0
        && !has_capability(&credentials, CAP_FSETID)
        && !is_group_member(&credentials, gid)
    {
        mode & !S_ISGID
    } else {
        mode
    }
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
        open_path_nofollow(path.clone())
    } else {
        open_path(path.clone())
    }?;
    if mode & 2 != 0 {
        VirtualFS.lock().ensure_writable_mount(path)?;
    }
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
            return Err(SyscallError::FileNotFound);
        }
        chmod_fd_object(
            get_object_current_process(dirfd as u64).map_err(SyscallError::from)?,
            mode,
        )?;
        return Ok(0);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let credentials = fs_access_credentials();
    check_access_path_search_permissions(&path, &credentials)?;
    if let Some(object) = proc_self_fd_object(&path)?
        && let Ok(file_like) = object.as_file_like()
        && file_like.read_link().is_ok()
    {
        return Err(SyscallError::OperationNotSupported);
    }
    let file = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path.clone())?
    } else {
        open_path(path.clone())?
    };
    if flags.contains(AtFlags::SYMLINK_NOFOLLOW)
        && matches!(file.info()?.file_like_type, FileLikeType::Symlink)
    {
        return Err(SyscallError::OperationNotSupported);
    }

    chmod_file_like(&file, Some(path), mode)?;
    Ok(0)
}

pub(in crate::systemcall::implementations::filesystem) fn chmod_fd_object(
    object: ObjectRef,
    mode: u32,
) -> Result<(), SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        if file_like.read_link().is_ok() {
            return Err(SyscallError::OperationNotSupported);
        }
        chmod_file_like(&file_like, None, mode)?;
    } else {
        let _ = object.as_statable()?;
    }

    Ok(())
}

fn chmod_file_like(
    file_like: &FileLikeObject,
    path: Option<Path>,
    mode: u32,
) -> Result<(), SyscallError> {
    let stat = file_like.stat();
    let credentials = fs_access_credentials();

    ensure_file_like_writable(file_like, path)?;

    if credentials.uid != stat.st_uid && !has_capability(&credentials, CAP_FOWNER) {
        return Err(SyscallError::PermissionDenied);
    }

    let mut mode = mode & 0o7777;
    if (mode & S_ISGID) != 0
        && !has_capability(&credentials, CAP_FSETID)
        && !is_group_member(&credentials, stat.st_gid)
    {
        mode &= !S_ISGID;
    }

    file_like.chmod(mode)?;
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
            return Err(SyscallError::FileNotFound);
        }
        chown_fd_object(
            get_object_current_process(dirfd as u64).map_err(SyscallError::from)?,
            uid,
            gid,
        )?;
        return Ok(0);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let credentials = fs_access_credentials();
    check_access_path_search_permissions(&path, &credentials)?;
    let file = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path.clone())?
    } else {
        open_path(path.clone())?
    };
    if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        chown_file_like(&file, Some(path), uid, gid, false)?;
    } else {
        chown_file_like(&file, Some(path), uid, gid, true)?;
    }
    Ok(0)
}

pub(in crate::systemcall::implementations::filesystem) fn chown_fd_object(
    object: ObjectRef,
    uid: u32,
    gid: u32,
) -> Result<(), SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        chown_file_like(&file_like, None, uid, gid, true)?;
    } else {
        let _ = object.as_statable()?;
    }

    Ok(())
}

fn chown_file_like(
    file_like: &FileLikeObject,
    path: Option<Path>,
    uid: u32,
    gid: u32,
    follow_symlink: bool,
) -> Result<(), SyscallError> {
    let stat = file_like.stat();
    let credentials = fs_access_credentials();
    let has_chown = has_capability(&credentials, CAP_CHOWN);

    ensure_file_like_writable(file_like, path)?;

    if !has_chown {
        if uid != u32::MAX && uid != stat.st_uid {
            return Err(SyscallError::PermissionDenied);
        }
        if credentials.uid != stat.st_uid {
            return Err(SyscallError::PermissionDenied);
        }
        if gid != u32::MAX && !is_group_member(&credentials, gid) {
            return Err(SyscallError::PermissionDenied);
        }
    }

    if follow_symlink {
        file_like.chown(uid, gid)?;
    } else {
        file_like.lchown(uid, gid)?;
    }

    clear_chown_mode_bits(file_like, stat.st_mode, &credentials)?;
    Ok(())
}

fn clear_chown_mode_bits(
    file_like: &FileLikeObject,
    old_mode: u32,
    credentials: &AccessCredentials,
) -> Result<(), SyscallError> {
    if old_mode & S_IFMT != S_IFREG {
        return Ok(());
    }
    let mut mode = old_mode & 0o7777;
    mode &= !S_ISUID;
    if !has_capability(credentials, CAP_FSETID) || (mode & S_IXGRP) != 0 {
        mode &= !S_ISGID;
    }
    if mode != (old_mode & 0o7777) {
        file_like.chmod(mode)?;
    }
    Ok(())
}

fn ensure_file_like_writable(
    file_like: &FileLikeObject,
    _path: Option<Path>,
) -> Result<(), SyscallError> {
    if file_like
        .mount_flags()
        .contains(crate::filesystem::vfs_traits::MountFlags::MS_RDONLY)
    {
        return Err(SyscallError::ReadOnlyFileSystem);
    }
    Ok(())
}
