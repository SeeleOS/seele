use super::*;
use crate::filesystem::vfs::MountPropagationUpdate;

const CAP_SYS_ADMIN: u64 = 21;
const LEGACY_MS_NOSYMFOLLOW: u64 = 1 << 8;

pub(super) fn require_cap_sys_admin() -> Result<(), SyscallError> {
    let credentials = fs_access_credentials();
    if has_capability(&credentials, CAP_SYS_ADMIN) {
        Ok(())
    } else {
        Err(SyscallError::PermissionDenied)
    }
}

pub(super) fn is_supported_api_mount(fstype: &str) -> bool {
    is_supported_fs_context_type(fstype) || matches!(fstype, "fuse" | "fuseblk")
}

pub(super) fn is_proc_fd_path(path: &str) -> bool {
    let path = Path::new(path).normalize().as_string();
    path.starts_with("/proc/") && path.contains("/fd/")
}

pub(super) fn is_char_device_mode(mode: u32) -> bool {
    mode & S_IFMT == S_IFCHR || mode != 0 && mode & S_IFMT == 0 && mode & S_IFCHR == S_IFCHR
}

pub(super) fn is_supported_fs_context_type(fstype: &str) -> bool {
    matches!(
        fstype,
        "proc"
            | "sysfs"
            | "devtmpfs"
            | "tmpfs"
            | "devpts"
            | "cgroup2"
            | "bpf"
            | "pstore"
            | "securityfs"
            | "ext2"
            | "ext3"
            | "ext4"
            | "ramfs"
    )
}

pub(super) fn create_api_filesystem(fstype: &str) -> Result<FileSystemRef, SyscallError> {
    let fs: FileSystemRef = match fstype {
        "proc" => Arc::new(Mut::new(ProcFs::new())),
        "sysfs" => Arc::new(Mut::new(SysFs::new())),
        "devtmpfs" => Arc::new(Mut::new(DevFs::new())),
        "tmpfs" => Arc::new(Mut::new(TmpFs::new())),
        "devpts" => Arc::new(Mut::new(DevPtsFs::new())),
        "cgroup2" => Arc::new(Mut::new(CgroupFs::new())),
        "bpf" | "pstore" | "securityfs" => Arc::new(Mut::new(TmpFs::new())),
        _ => return Err(SyscallError::NoDevice),
    };
    Ok(fs)
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FuseMountOptions {
    pub(super) fd: Option<u64>,
}
pub(super) fn parse_fuse_mount_options(
    data: Option<&str>,
) -> Result<FuseMountOptions, SyscallError> {
    let mut options = FuseMountOptions::default();
    let Some(data) = data else {
        return Ok(options);
    };

    for item in data.split(',').filter(|item| !item.is_empty()) {
        let Some((key, value)) = item.split_once('=') else {
            continue;
        };
        if key == "fd" {
            options.fd = Some(
                value
                    .parse::<u64>()
                    .map_err(|_| SyscallError::InvalidArguments)?,
            );
        }
    }

    Ok(options)
}
pub(super) fn ensure_directory_exists(path: &str) -> Result<(), SyscallError> {
    let path = Path::new(path);
    if let Ok(info) = file_info_path(path.clone()) {
        return match info.file_like_type {
            FileLikeType::Directory => Ok(()),
            _ => Err(SyscallError::NotADirectory),
        };
    }
    VirtualFS.lock().create_dir(path)?;
    Ok(())
}

pub(super) fn next_api_mount_path() -> Result<Path, SyscallError> {
    ensure_directory_exists(API_MOUNT_ROOT)?;
    let mount_id = NEXT_API_MOUNT_ID.fetch_add(1, Ordering::Relaxed);
    let path = Path::new(&format!("{API_MOUNT_ROOT}/{mount_id}"));
    ensure_directory_exists(&path.clone().as_string())?;
    Ok(path)
}
pub(super) fn is_api_mount_path(path: &Path) -> bool {
    path.clone()
        .as_string()
        .starts_with(&(String::from(API_MOUNT_ROOT) + "/"))
}

pub(super) fn remount_bind_flag_update(bits: u64) -> (MountFlags, MountFlags) {
    let flags = mount_flags_from_mount_bits(bits);
    let mut mask = MountFlags::MS_RDONLY
        | MountFlags::MS_NOSUID
        | MountFlags::MS_NODEV
        | MountFlags::MS_NOEXEC;
    if flags.contains(MountFlags::MS_RELATIME) {
        mask |= MountFlags::MS_RELATIME;
    }
    if bits & LEGACY_MS_NOSYMFOLLOW != 0 {
        mask |= MountFlags::MS_NOSYMFOLLOW;
    }
    (flags, mask)
}

pub(super) fn mount_flags_from_mount_bits(bits: u64) -> MountFlags {
    let mut flags = MountFlags::from_bits_retain(bits & MountFlags::all().bits());
    if bits & LEGACY_MS_NOSYMFOLLOW != 0 {
        flags.insert(MountFlags::MS_NOSYMFOLLOW);
    }
    flags
}

pub(super) fn apply_initial_mount_flags(path: Path, flags: MountFlags) -> Result<(), SyscallError> {
    let mask = MountFlags::MS_RDONLY
        | MountFlags::MS_NOSUID
        | MountFlags::MS_NODEV
        | MountFlags::MS_NOEXEC
        | MountFlags::MS_RELATIME
        | MountFlags::MS_NOATIME
        | MountFlags::MS_STRICTATIME
        | MountFlags::MS_NODIRATIME
        | MountFlags::MS_NOSYMFOLLOW;
    VirtualFS
        .lock()
        .remount_bind(path, flags, mask, false)
        .map_err(SyscallError::from)
}

pub(super) fn ensure_mount_root(path: &Path) -> Result<(), SyscallError> {
    let mount_path = VirtualFS
        .lock()
        .mount_path(path.clone())
        .map_err(SyscallError::from)?;
    if mount_path == *path {
        Ok(())
    } else {
        Err(SyscallError::InvalidArguments)
    }
}

pub(super) fn mount_propagation_from_mount_flags(
    flags: MountOperationFlags,
) -> Option<MountPropagationUpdate> {
    let mut propagation = None;
    for (flag, update) in [
        (
            MountOperationFlags::MS_PRIVATE,
            MountPropagationUpdate::Private,
        ),
        (
            MountOperationFlags::MS_SHARED,
            MountPropagationUpdate::Shared,
        ),
        (MountOperationFlags::MS_SLAVE, MountPropagationUpdate::Slave),
        (
            MountOperationFlags::MS_UNBINDABLE,
            MountPropagationUpdate::Unbindable,
        ),
    ] {
        if flags.contains(flag) {
            if propagation.is_some() {
                return None;
            }
            propagation = Some(update);
        }
    }
    propagation
}

pub(super) fn mount_attr_flag_update(
    attr: &LinuxMountAttr,
) -> Result<(MountFlags, MountFlags, Option<MountPropagationUpdate>), SyscallError> {
    let supported_basic =
        MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV | MOUNT_ATTR_NOEXEC;
    let supported_set = supported_basic
        | MOUNT_ATTR_NOATIME
        | MOUNT_ATTR_STRICTATIME
        | MOUNT_ATTR_NODIRATIME
        | MOUNT_ATTR_NOSYMFOLLOW;
    let supported_clr = supported_basic | MOUNT_ATTR__ATIME;

    let propagation = match attr.propagation {
        0 => None,
        value if value == MountOperationFlags::MS_PRIVATE.bits() => {
            Some(MountPropagationUpdate::Private)
        }
        value if value == MountOperationFlags::MS_SHARED.bits() => {
            Some(MountPropagationUpdate::Shared)
        }
        value if value == MountOperationFlags::MS_SLAVE.bits() => {
            Some(MountPropagationUpdate::Slave)
        }
        value if value == MountOperationFlags::MS_UNBINDABLE.bits() => {
            Some(MountPropagationUpdate::Unbindable)
        }
        _ => return Err(SyscallError::InvalidArguments),
    };
    if attr.attr_set & !supported_set != 0 {
        return Err(SyscallError::OperationNotSupported);
    }
    if attr.attr_clr & !supported_clr != 0 {
        return Err(SyscallError::OperationNotSupported);
    }
    if attr.attr_set & MOUNT_ATTR_IDMAP != 0 {
        return Err(SyscallError::OperationNotSupported);
    }

    let basic_mask = MountFlags::MS_RDONLY
        | MountFlags::MS_NOSUID
        | MountFlags::MS_NODEV
        | MountFlags::MS_NOEXEC;
    let mut flags = MountFlags::from_bits_retain(attr.attr_set & basic_mask.bits());
    let mut mask =
        MountFlags::from_bits_retain((attr.attr_set | attr.attr_clr) & basic_mask.bits());

    if attr.attr_clr & MOUNT_ATTR__ATIME != 0 {
        flags.insert(MountFlags::MS_RELATIME);
        mask.insert(MountFlags::MS_NOATIME | MountFlags::MS_RELATIME | MountFlags::MS_STRICTATIME);
    }
    if attr.attr_set & MOUNT_ATTR_NOATIME != 0 {
        flags.insert(MountFlags::MS_NOATIME);
        mask.insert(MountFlags::MS_NOATIME | MountFlags::MS_RELATIME | MountFlags::MS_STRICTATIME);
    }
    if attr.attr_set & MOUNT_ATTR_STRICTATIME != 0 {
        flags.insert(MountFlags::MS_STRICTATIME);
        mask.insert(MountFlags::MS_NOATIME | MountFlags::MS_RELATIME | MountFlags::MS_STRICTATIME);
    }
    if attr.attr_set & MOUNT_ATTR_NODIRATIME != 0 {
        flags.insert(MountFlags::MS_NODIRATIME);
        mask.insert(MountFlags::MS_NODIRATIME);
    }
    if attr.attr_set & MOUNT_ATTR_NOSYMFOLLOW != 0 {
        flags.insert(MountFlags::MS_NOSYMFOLLOW);
        mask.insert(MountFlags::MS_NOSYMFOLLOW);
    }

    Ok((flags, mask, propagation))
}

pub(super) fn tmpfs_root_mode_from_mount_data(
    data: Option<&str>,
) -> Result<Option<u32>, SyscallError> {
    let Some(data) = data else {
        return Ok(None);
    };

    for option in data.split(',') {
        let Some(value) = option.strip_prefix("mode=") else {
            continue;
        };
        return Ok(Some(
            u32::from_str_radix(value, 8).map_err(|_| SyscallError::InvalidArguments)?,
        ));
    }

    Ok(None)
}

pub(super) fn mount_setattr_target_path(
    dirfd: i32,
    path: CString,
    flags: AtFlags,
) -> Result<Path, SyscallError> {
    if path.is_null() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::BadAddress);
        }
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return Ok(object.as_file_like()?.path().normalize());
    }

    let path = path_from_raw(path)?;
    if path.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return Ok(object.as_file_like()?.path().normalize());
    }

    resolve_path_at(dirfd, &path).map(|path| path.normalize())
}
pub(super) fn validate_umount_flags(flags: UmountFlags) -> Result<UmountFlags, SyscallError> {
    if flags.bits()
        != flags.bits()
            & (UmountFlags::FORCE
                | UmountFlags::DETACH
                | UmountFlags::EXPIRE
                | UmountFlags::NOFOLLOW)
                .bits()
    {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(flags)
}
