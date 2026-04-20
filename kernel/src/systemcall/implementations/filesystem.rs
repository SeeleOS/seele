use crate::{
    define_syscall,
    filesystem::{
        errors::FSError,
        info::LinuxStat,
        misc::smart_resolve_path,
        object::FileLikeObject,
        path::Path,
        tmpfs::TmpFs,
        vfs::VirtualFS,
        vfs_traits::{FileLikeType, MountFlags},
    },
    memory::user_safe,
    misc::{c_types::CString, others::KernelFrom},
    object::{
        FileFlags, Object,
        error::ObjectError,
        fs_context::{FsConfigCommand, FsContextObject},
        misc::{ObjectRef, get_object_current_process},
    },
    process::{FdFlags, manager::get_current_process, misc::with_current_process},
    systemcall::utils::{SyscallError, SyscallImpl},
};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;
use core::sync::atomic::{AtomicU64, Ordering};

const AT_FDCWD: i32 = -100;
const UTIME_OMIT: i64 = 0x3fff_ffff;
const STATX_BASIC_STATS: u32 = 0x0000_07ff;
const STATX_MNT_ID: u32 = 0x0000_1000;
const STATX_ATTR_MOUNT_ROOT: u64 = 0x0000_2000;
const ANON_INODE_FS_MAGIC: i64 = 0x0904_1934;
const SOCKFS_MAGIC: i64 = 0x534f_434b;
const AT_STATX_FORCE_SYNC: i32 = 0x2000;
const AT_STATX_DONT_SYNC: i32 = 0x4000;
const S_IFMT: u32 = 0o170000;
const S_IFREG: u32 = 0o100000;
const S_IFIFO: u32 = 0o010000;
const S_IFCHR: u32 = 0o020000;
const S_IFBLK: u32 = 0o060000;
const S_IFSOCK: u32 = 0o140000;
const API_MOUNT_ROOT: &str = "/run/.api-mounts";

static NEXT_API_MOUNT_ID: AtomicU64 = AtomicU64::new(1);

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct AtFlags: i32 {
        const REMOVEDIR = 0x200;
        const SYMLINK_NOFOLLOW = 0x100;
        const SYMLINK_FOLLOW = 0x400;
        const NO_AUTOMOUNT = 0x800;
        const EMPTY_PATH = 0x1000;
        const STATX_FORCE_SYNC = 0x2000;
        const STATX_DONT_SYNC = 0x4000;
        const EACCESS = 0x200;
        const RECURSIVE = 0x8000;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct StatxTimestamp {
    tv_sec: i64,
    tv_nsec: u32,
    __reserved: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxStatx {
    stx_mask: u32,
    stx_blksize: u32,
    stx_attributes: u64,
    stx_nlink: u32,
    stx_uid: u32,
    stx_gid: u32,
    stx_mode: u16,
    __spare0: u16,
    stx_ino: u64,
    stx_size: u64,
    stx_blocks: u64,
    stx_attributes_mask: u64,
    stx_atime: StatxTimestamp,
    stx_btime: StatxTimestamp,
    stx_ctime: StatxTimestamp,
    stx_mtime: StatxTimestamp,
    stx_rdev_major: u32,
    stx_rdev_minor: u32,
    stx_dev_major: u32,
    stx_dev_minor: u32,
    stx_mnt_id: u64,
    stx_dio_mem_align: u32,
    stx_dio_offset_align: u32,
    __spare3: [u64; 12],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxStatFs {
    f_type: i64,
    f_bsize: i64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_fsid: i64,
    f_namelen: i64,
    f_frsize: i64,
    f_flags: i64,
    f_spare: [i64; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxMountAttr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct OpenFlags: i32 {
        const CREAT = 0x40;
        const EXCL = 0x80;
        const NOCTTY = 0x100;
        const TRUNC = 0x200;
        const APPEND = 0o2_000;
        const NONBLOCK = 0o4_000;
        const DSYNC = 0o10_000;
        const DIRECT = 0o40_000;
        const LARGEFILE = 0o100_000;
        const DIRECTORY = 0o200000;
        const NOFOLLOW = 0o400000;
        const NOATIME = 0o1000000;
        const CLOEXEC = 0o2000000;
        const SYNC = 0x101000;
        const PATH = 0o10000000;
        const TMPFILE = 0o20200000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct XattrFlags: u32 {
        const CREATE = 0x1;
        const REPLACE = 0x2;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct UmountFlags: i32 {
        const FORCE = 0x1;
        const DETACH = 0x2;
        const EXPIRE = 0x4;
        const NOFOLLOW = 0x8;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct FsOpenFlags: u32 {
        const FSCONTEXT_CLOEXEC = 0x1;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct FsMountFlags: u32 {
        const FSMOUNT_CLOEXEC = 0x1;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct MoveMountFlags: u32 {
        const MOVE_MOUNT_F_SYMLINKS = 0x0000_0001;
        const MOVE_MOUNT_F_AUTOMOUNTS = 0x0000_0002;
        const MOVE_MOUNT_F_EMPTY_PATH = 0x0000_0004;
        const MOVE_MOUNT_T_SYMLINKS = 0x0000_0010;
        const MOVE_MOUNT_T_AUTOMOUNTS = 0x0000_0020;
        const MOVE_MOUNT_T_EMPTY_PATH = 0x0000_0040;
        const MOVE_MOUNT_SET_GROUP = 0x0000_0100;
        const MOVE_MOUNT_BENEATH = 0x0000_0200;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct OpenTreeFlags: u32 {
        const OPEN_TREE_CLONE = 0x0000_0001;
        const AT_SYMLINK_NOFOLLOW = AtFlags::SYMLINK_NOFOLLOW.bits() as u32;
        const AT_NO_AUTOMOUNT = AtFlags::NO_AUTOMOUNT.bits() as u32;
        const AT_EMPTY_PATH = AtFlags::EMPTY_PATH.bits() as u32;
        const OPEN_TREE_CLOEXEC = 0x0008_0000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct MountOperationFlags: u64 {
        const MS_REMOUNT = 32;
        const MS_BIND = 4096;
        const MS_MOVE = 8192;
        const MS_REC = 16384;
        const MS_UNBINDABLE = 1 << 17;
        const MS_PRIVATE = 1 << 18;
        const MS_SLAVE = 1 << 19;
        const MS_SHARED = 1 << 20;
    }
}

fn path_from_raw(path: CString) -> Result<String, SyscallError> {
    if path.is_null() {
        return Err(SyscallError::BadAddress);
    }

    String::k_from(path).map_err(|_| SyscallError::InvalidArguments)
}

fn string_from_raw_optional(value: CString) -> Result<Option<String>, SyscallError> {
    if value.is_null() {
        return Ok(None);
    }

    String::k_from(value)
        .map(Some)
        .map_err(|_| SyscallError::InvalidArguments)
}

fn is_supported_api_mount(fstype: &str) -> bool {
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
    )
}

fn path_is_relative_to_cwd(dirfd: i32) -> Result<bool, SyscallError> {
    match dirfd {
        AT_FDCWD => Ok(true),
        _ => Err(SyscallError::NoSyscall),
    }
}

fn resolve_path_at(dirfd: i32, path_str: &str) -> Result<Path, SyscallError> {
    if path_str.starts_with('/') {
        return Ok(Path::new(path_str));
    }

    if dirfd == AT_FDCWD {
        let mut current_dir = with_current_process(|process| process.current_directory.clone());
        current_dir.push_path_str(path_str);
        return Ok(current_dir.as_normal());
    }

    let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
    let file_like = object.as_file_like()?;
    if !matches!(file_like.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }

    let mut base = file_like.path().as_absolute();
    base.push_path_str(path_str);
    Ok(base.as_normal())
}

fn ensure_directory_exists(path: &str) -> Result<(), SyscallError> {
    let path = Path::new(path);
    if VirtualFS.lock().file_info(path.clone()).is_ok() {
        return Ok(());
    }
    VirtualFS.lock().create_dir(path)?;
    Ok(())
}

fn next_api_mount_path() -> Result<Path, SyscallError> {
    ensure_directory_exists(API_MOUNT_ROOT)?;
    let mount_id = NEXT_API_MOUNT_ID.fetch_add(1, Ordering::Relaxed);
    let path = Path::new(&format!("{API_MOUNT_ROOT}/{mount_id}"));
    ensure_directory_exists(&path.clone().as_string())?;
    Ok(path)
}

fn is_api_mount_path(path: &Path) -> bool {
    path.clone()
        .as_string()
        .starts_with(&(String::from(API_MOUNT_ROOT) + "/"))
}

fn should_trace_namespace_path(path: &str) -> bool {
    let _ = path;
    false
}

fn current_process_is_executor() -> bool {
    false
}

fn current_process_is_journald() -> bool {
    with_current_process(|process| {
        process
            .command_line
            .first()
            .is_some_and(|path| path.ends_with("/systemd-journald"))
    })
}

fn remount_bind_flag_update(bits: u64) -> (MountFlags, MountFlags) {
    let flags = MountFlags::from_bits_retain(bits & MountFlags::all().bits());
    let mut mask = MountFlags::MS_RDONLY
        | MountFlags::MS_NOSUID
        | MountFlags::MS_NODEV
        | MountFlags::MS_NOEXEC;
    if flags.contains(MountFlags::MS_RELATIME) {
        mask |= MountFlags::MS_RELATIME;
    }
    (flags, mask)
}

fn symlink_target_matches(path: &Path, expected_target: &str) -> bool {
    let Ok(link) = VirtualFS.lock().open_nofollow(path.clone()) else {
        return false;
    };
    let Ok(target) = link.read_link() else {
        return false;
    };
    target == expected_target
}

fn check_access_mode(mode: i32) -> Result<(), SyscallError> {
    if (mode & !7) != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(())
}

fn check_access_permissions(stat: &LinuxStat, mode: i32) -> Result<(), SyscallError> {
    let permission = stat.st_mode & 0o777;

    if (mode & 4) != 0 && permission & 0o444 == 0 {
        return Err(SyscallError::PermissionDenied);
    }
    if (mode & 2) != 0 && permission & 0o222 == 0 {
        return Err(SyscallError::PermissionDenied);
    }
    if (mode & 1) != 0 && permission & 0o111 == 0 {
        return Err(SyscallError::PermissionDenied);
    }

    Ok(())
}

fn linux_major(dev: u64) -> u32 {
    (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)) as u32
}

fn linux_minor(dev: u64) -> u32 {
    ((dev & 0xff) | ((dev >> 12) & !0xff)) as u32
}

fn filesystem_magic_for_file_like(file_like: &FileLikeObject) -> Result<i64, SyscallError> {
    filesystem_magic_for_path(&file_like.path())
}

fn filesystem_magic_for_object(object: &ObjectRef) -> Result<i64, SyscallError> {
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

    if object.clone().as_netlink_socket().is_ok() || object.clone().as_unix_socket().is_ok() {
        return Ok(SOCKFS_MAGIC);
    }

    Err(SyscallError::BadFileDescriptor)
}

fn filesystem_magic_for_path(path: &Path) -> Result<i64, SyscallError> {
    let (_mount_path, fs, _, _) = VirtualFS.lock().mount_metadata(path.clone())?;
    Ok(fs.lock().magic())
}

fn mount_id_for_path(path: &Path) -> Result<u64, SyscallError> {
    let mount_path = VirtualFS.lock().mount_path(path.clone())?.as_string();
    let mut mounts = VirtualFS
        .lock()
        .mount_snapshots()
        .into_iter()
        .map(|(path, _, _, _)| path.as_string())
        .collect::<Vec<_>>();
    mounts.sort_by_key(|path| (path.matches('/').count(), path.len()));
    Ok(mounts
        .iter()
        .position(|path| *path == mount_path)
        .map(|index| index as u64 + 1)
        .unwrap_or(1))
}

fn mount_id_for_file_like(file_like: &FileLikeObject) -> Result<u64, SyscallError> {
    mount_id_for_path(&file_like.path())
}

fn stat_mount_id_at(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<u64, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        let file_like = object.as_file_like()?;
        return mount_id_for_file_like(&file_like);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    mount_id_for_path(&path)
}

fn stat_mount_root_at(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<bool, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        let file_like = object.as_file_like()?;
        let path = file_like.path().normalize();
        let mount_path = VirtualFS.lock().mount_path(path.clone())?;
        return Ok(path.as_string() == mount_path.as_string());
    }

    let path = resolve_path_at(dirfd, path_str)?.normalize();
    let mount_path = VirtualFS.lock().mount_path(path.clone())?;
    Ok(path.as_string() == mount_path.as_string())
}

#[repr(C)]
struct LinuxFileHandle {
    handle_bytes: u32,
    handle_type: i32,
    f_handle: [u8; 0],
}

fn linux_statfs(f_type: i64) -> LinuxStatFs {
    LinuxStatFs {
        f_type,
        f_bsize: 4096,
        f_blocks: 262_144,
        f_bfree: 131_072,
        f_bavail: 131_072,
        f_files: 262_144,
        f_ffree: 131_072,
        f_fsid: 1,
        f_namelen: 255,
        f_frsize: 4096,
        f_flags: 0,
        f_spare: [0; 4],
    }
}

fn readlink_impl(path: Path, out_buf: *mut u8, out_len: usize) -> Result<usize, SyscallError> {
    let target = match VirtualFS.lock().open_nofollow(path)?.read_link() {
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

fn xattr_name_from_raw(name: CString) -> Result<String, SyscallError> {
    path_from_raw(name)
}

fn ensure_path_exists_at(dirfd: i32, path_str: &str, nofollow: bool) -> Result<(), SyscallError> {
    let path = resolve_path_at(dirfd, path_str)?;
    let _ = if nofollow {
        VirtualFS.lock().open_nofollow(path)?
    } else {
        VirtualFS.lock().open(path)?
    };
    Ok(())
}

fn ensure_object_supports_xattrs(object: &ObjectRef) -> Result<(), SyscallError> {
    let _ = object.clone().as_file_like()?;
    Ok(())
}

fn validate_xattr_flags(flags: XattrFlags) -> Result<(), SyscallError> {
    if flags.bits() != flags.bits() & (XattrFlags::CREATE | XattrFlags::REPLACE).bits() {
        return Err(SyscallError::InvalidArguments);
    }
    if flags.contains(XattrFlags::CREATE) && flags.contains(XattrFlags::REPLACE) {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(())
}

fn validate_umount_flags(flags: UmountFlags) -> Result<UmountFlags, SyscallError> {
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

fn faccessat_impl(
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
    if should_trace_namespace_path(&path.clone().as_string()) {
        crate::s_println!("faccessat resolved {}", path.clone().as_string());
    }
    let open_result = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        VirtualFS.lock().open_nofollow(path)
    } else {
        VirtualFS.lock().open(path)
    };
    let object: ObjectRef = Arc::new(open_result?);
    check_access_permissions(&object.as_statable()?.stat(), mode)?;
    Ok(0)
}

fn rename_impl(
    old_from_currentdir: bool,
    old_path: String,
    new_from_currentdir: bool,
    new_path: String,
) -> Result<usize, SyscallError> {
    let old_path =
        smart_resolve_path(old_path, old_from_currentdir).ok_or(SyscallError::InvalidArguments)?;
    let new_path =
        smart_resolve_path(new_path, new_from_currentdir).ok_or(SyscallError::InvalidArguments)?;

    if old_path.clone().as_string() == new_path.clone().as_string() {
        return Ok(0);
    }

    VirtualFS.lock().rename_file(old_path, new_path)?;
    Ok(0)
}

fn stat_at(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<LinuxStat, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return Ok(object.as_statable()?.stat());
    }

    if should_trace_namespace_path(path_str) || current_process_is_executor() {
        crate::s_println!(
            "stat trace dirfd={} path={} flags={:#x}",
            dirfd,
            path_str,
            flags.bits()
        );
    }
    let path = resolve_path_at(dirfd, path_str)?;
    if should_trace_namespace_path(&path.clone().as_string()) {
        crate::s_println!("stat resolved {}", path.clone().as_string());
    }
    let open_result = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        VirtualFS.lock().open_nofollow(path.clone())
    } else {
        VirtualFS.lock().open(path.clone())
    };
    let object: ObjectRef = Arc::new(open_result?);
    let stat = object.as_statable()?.stat();
    Ok(stat)
}

fn chmod_at(dirfd: i32, path_str: &str, mode: u32, flags: AtFlags) -> Result<usize, SyscallError> {
    let allowed_flags = AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW;
    if flags.bits() != (flags & allowed_flags).bits() {
        return Err(SyscallError::InvalidArguments);
    }

    let mode = mode & !S_IFMT;
    if path_str.is_empty() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }
        get_object_current_process(dirfd as u64)
            .map_err(SyscallError::from)?
            .as_file_like()?
            .chmod(mode)?;
        return Ok(0);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let file = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        VirtualFS.lock().open_nofollow(path)?
    } else {
        VirtualFS.lock().open(path)?
    };
    if flags.contains(AtFlags::SYMLINK_NOFOLLOW)
        && matches!(file.info()?.file_like_type, FileLikeType::Symlink)
    {
        return Err(SyscallError::OperationNotSupported);
    }

    file.chmod(mode)?;
    Ok(0)
}

define_syscall!(OpenAt, |dirfd: i32,
                         path: CString,
                         flags: OpenFlags,
                         _mode: u32| {
    let current_process = get_current_process();
    let path_str = path_from_raw(path)?;
    if should_trace_namespace_path(&path_str) || current_process_is_executor() {
        crate::s_println!(
            "openat trace dirfd={} path={} flags={:#x}",
            dirfd,
            path_str,
            flags.bits()
        );
    }
    if flags.contains(OpenFlags::TMPFILE) {
        return Err(SyscallError::OperationNotSupported);
    }
    let create = flags.contains(OpenFlags::CREAT);
    let nofollow = flags.contains(OpenFlags::NOFOLLOW);
    let directory_only = flags.contains(OpenFlags::DIRECTORY);
    let path_only = flags.contains(OpenFlags::PATH);
    let trace_journald = current_process_is_journald();

    let path = match resolve_path_at(dirfd, &path_str) {
        Ok(path) => path,
        Err(err) => {
            if trace_journald {
                crate::s_println!(
                    "journald openat path error: dirfd={} path={} flags={:#x} err={:?}",
                    dirfd,
                    path_str,
                    flags.bits(),
                    err
                );
            }
            return Err(err);
        }
    };
    if should_trace_namespace_path(&path.clone().as_string()) {
        crate::s_println!("openat resolved {}", path.clone().as_string());
    }
    let open_result = if nofollow {
        VirtualFS.lock().open_nofollow(path.clone())
    } else {
        VirtualFS.lock().open(path.clone())
    };
    let object;
    if let Ok(file) = open_result {
        if create && flags.contains(OpenFlags::EXCL) {
            return Err(SyscallError::FileAlreadyExists);
        }
        object = Arc::new(file);
    } else if create {
        let create_result = VirtualFS.lock().create_file(path.clone());
        if let Err(err) = create_result {
            if trace_journald {
                crate::s_println!(
                    "journald openat create error: path={} flags={:#x} err={:?}",
                    path.as_string(),
                    flags.bits(),
                    err
                );
            }
            return Err(err.into());
        }
        let reopen_result = VirtualFS.lock().open(path.clone());
        object = match reopen_result {
            Ok(file) => Arc::new(file),
            Err(err) => {
                if trace_journald {
                    crate::s_println!(
                        "journald openat reopen error: path={} flags={:#x} err={:?}",
                        path.as_string(),
                        flags.bits(),
                        err
                    );
                }
                return Err(err.into());
            }
        };
    } else {
        if trace_journald {
            crate::s_println!(
                "journald openat missing: path={} flags={:#x}",
                path.as_string(),
                flags.bits()
            );
        }
        return Err(SyscallError::FileNotFound);
    }

    let info = object.info()?;
    if nofollow && !path_only && matches!(info.file_like_type, FileLikeType::Symlink) {
        return Err(SyscallError::TooManySymbolicLinks);
    }
    if directory_only && !matches!(info.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }
    if flags.contains(OpenFlags::NONBLOCK) {
        match object.clone().set_flags(FileFlags::NONBLOCK) {
            Ok(()) | Err(ObjectError::Unimplemented) => {}
            Err(err) => return Err(err.into()),
        }
    }

    let fd_flags = if flags.contains(OpenFlags::CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    Ok(current_process
        .lock()
        .push_object_with_flags(object, fd_flags))
});

define_syscall!(Open, |path: CString, flags: OpenFlags, mode: u32| {
    OpenAt::handle_call(
        (-100i32) as u64,
        path as u64,
        flags.bits() as u64,
        mode as u64,
        0,
        0,
    )
});

define_syscall!(Access, |path: CString, mode: i32| {
    check_access_mode(mode)?;
    let path_str = path_from_raw(path)?;
    if should_trace_namespace_path(&path_str) || current_process_is_executor() {
        crate::s_println!("access trace path={} mode={:#x}", path_str, mode);
    }
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    if should_trace_namespace_path(&path.clone().as_string()) {
        crate::s_println!("access resolved {}", path.clone().as_string());
    }
    let open_result = VirtualFS.lock().open(path);
    let _ = open_result?;
    Ok(0)
});

define_syscall!(Chdir, |dir: String| {
    let path = Path::new(&dir).as_absolute();
    get_current_process().lock().change_directory(path)?;
    Ok(0)
});

define_syscall!(Fchdir, |fd: u64| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    let file_like = object.as_file_like()?;
    if !matches!(file_like.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }

    get_current_process()
        .lock()
        .change_directory(file_like.path().as_absolute())?;
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
    let file = VirtualFS.lock().open(path)?;
    file.chmod(mode)?;
    Ok(0)
});

define_syscall!(Chown, |path: CString, _owner: u32, _group: u32| {
    let path_str = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    let _ = VirtualFS.lock().open(path)?;
    Ok(0)
});

define_syscall!(Getcwd, |buf_ptr: *mut u8, len: usize| {
    if buf_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let process = get_current_process();
    let path_str = process.lock().current_directory.clone().as_string();
    let path_bytes = path_str.as_bytes();
    let path_len = path_bytes.len();

    if len > path_len {
        let mut buffer = Vec::with_capacity(path_len + 1);
        buffer.extend_from_slice(path_bytes);
        buffer.push(0);
        user_safe::write(buf_ptr, &buffer[..])?;
    } else {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(path_len + 1)
});

define_syscall!(Fstat, |fd: u64, linux_stat_ptr: *mut LinuxStat| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    if current_process_is_executor()
        && let Ok(file_like) = object.clone().as_file_like()
    {
        let path = file_like.path().as_string();
        if path.ends_with("/mountinfo") {
            crate::s_println!("mountinfo object trace action=fstat path={}", path);
        }
    }
    let stat = object.as_statable()?.stat();
    user_safe::write(linux_stat_ptr, &stat)?;
    Ok(0)
});

define_syscall!(Fchmod, |fd: u64, mode: u32| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    object.as_file_like()?.chmod(mode)?;
    Ok(0)
});

define_syscall!(Fchmodat2, |dirfd: i32,
                            path: u64,
                            mode: u32,
                            flags: AtFlags| {
    let path = path as CString;
    let path_str = if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            String::new()
        } else {
            return Err(SyscallError::BadAddress);
        }
    } else {
        path_from_raw(path)?
    };

    chmod_at(dirfd, &path_str, mode, flags)
});

define_syscall!(Newfstatat, |dirfd: i32,
                             path: u64,
                             linux_stat_ptr: *mut LinuxStat,
                             flags: AtFlags| {
    let path = path as CString;
    if flags.bits()
        != flags.bits()
            & (AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT | AtFlags::EMPTY_PATH).bits()
    {
        return Err(SyscallError::NoSyscall);
    }
    let path_str = if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            String::new()
        } else {
            return Err(SyscallError::BadAddress);
        }
    } else {
        path_from_raw(path)?
    };

    let stat = match stat_at(dirfd, &path_str, flags) {
        Ok(stat) => stat,
        Err(err) => {
            if current_process_is_journald() {
                crate::s_println!(
                    "journald fstatat error: dirfd={} path={} flags={:#x} err={:?}",
                    dirfd,
                    path_str,
                    flags.bits(),
                    err
                );
            }
            return Err(err);
        }
    };

    user_safe::write(linux_stat_ptr, &stat)?;
    Ok(0)
});

define_syscall!(Statx, |dirfd: i32,
                        path: CString,
                        flags: AtFlags,
                        _mask: u32,
                        statx_ptr: *mut LinuxStatx| {
    let allowed_flags = (AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT | AtFlags::EMPTY_PATH)
        .bits()
        | AT_STATX_FORCE_SYNC
        | AT_STATX_DONT_SYNC;
    if flags.bits() != flags.bits() & allowed_flags {
        return Err(SyscallError::NoSyscall);
    }
    let path_str = if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            String::new()
        } else {
            return Err(SyscallError::BadAddress);
        }
    } else {
        path_from_raw(path)?
    };
    if statx_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let stat = stat_at(dirfd, &path_str, flags)?;
    let mount_id = stat_mount_id_at(dirfd, &path_str, flags)?;
    let mount_root = stat_mount_root_at(dirfd, &path_str, flags)?;

    let statx = LinuxStatx {
        stx_mask: STATX_BASIC_STATS | STATX_MNT_ID,
        stx_blksize: stat.st_blksize as u32,
        stx_attributes: if mount_root { STATX_ATTR_MOUNT_ROOT } else { 0 },
        stx_nlink: stat.st_nlink as u32,
        stx_uid: stat.st_uid,
        stx_gid: stat.st_gid,
        stx_mode: stat.st_mode as u16,
        stx_ino: stat.st_ino,
        stx_size: stat.st_size as u64,
        stx_blocks: stat.st_blocks as u64,
        stx_atime: StatxTimestamp {
            tv_sec: stat.st_atime,
            tv_nsec: stat.st_atime_nsec as u32,
            __reserved: 0,
        },
        stx_ctime: StatxTimestamp {
            tv_sec: stat.st_ctime,
            tv_nsec: stat.st_ctime_nsec as u32,
            __reserved: 0,
        },
        stx_mtime: StatxTimestamp {
            tv_sec: stat.st_mtime,
            tv_nsec: stat.st_mtime_nsec as u32,
            __reserved: 0,
        },
        stx_rdev_major: linux_major(stat.st_rdev),
        stx_rdev_minor: linux_minor(stat.st_rdev),
        stx_dev_major: linux_major(stat.st_dev),
        stx_dev_minor: linux_minor(stat.st_dev),
        stx_mnt_id: mount_id,
        stx_attributes_mask: STATX_ATTR_MOUNT_ROOT,
        ..Default::default()
    };
    user_safe::write(statx_ptr, &statx)?;

    Ok(0)
});

define_syscall!(Faccessat, |dirfd: i32,
                            path: CString,
                            mode: i32,
                            flags: AtFlags| {
    let path_str = path_from_raw(path)?;
    if should_trace_namespace_path(&path_str) || current_process_is_executor() {
        crate::s_println!(
            "faccessat trace dirfd={} path={} mode={:#x} flags={:#x}",
            dirfd,
            path_str,
            mode,
            flags.bits()
        );
    }
    faccessat_impl(dirfd, &path_str, mode, flags)
});

define_syscall!(Faccessat2, |dirfd: i32,
                             path: CString,
                             mode: i32,
                             flags: AtFlags| {
    let path_str = path_from_raw(path)?;
    if should_trace_namespace_path(&path_str) || current_process_is_executor() {
        crate::s_println!(
            "faccessat2 trace dirfd={} path={} mode={:#x} flags={:#x}",
            dirfd,
            path_str,
            mode,
            flags.bits()
        );
    }
    faccessat_impl(dirfd, &path_str, mode, flags)
});

define_syscall!(Getxattr, |path: CString,
                           name: CString,
                           _value: *mut u8,
                           _size: usize| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, false)?;
    Err(SyscallError::NoData)
});

define_syscall!(Lgetxattr, |path: CString,
                            name: CString,
                            _value: *mut u8,
                            _size: usize| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, true)?;
    Err(SyscallError::NoData)
});

define_syscall!(Fgetxattr, |object: ObjectRef,
                            name: CString,
                            _value: *mut u8,
                            _size: usize| {
    let _name = xattr_name_from_raw(name)?;
    ensure_object_supports_xattrs(&object)?;
    Err(SyscallError::NoData)
});

define_syscall!(Setxattr, |path: CString,
                           name: CString,
                           _value: *const u8,
                           _size: usize,
                           flags: XattrFlags| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, false)?;
    Ok(0)
});

define_syscall!(Lsetxattr, |path: CString,
                            name: CString,
                            _value: *const u8,
                            _size: usize,
                            flags: XattrFlags| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, true)?;
    Ok(0)
});

define_syscall!(Fsetxattr, |object: ObjectRef,
                            name: CString,
                            _value: *const u8,
                            _size: usize,
                            flags: XattrFlags| {
    let _name = xattr_name_from_raw(name)?;
    validate_xattr_flags(flags)?;
    ensure_object_supports_xattrs(&object)?;
    Ok(0)
});

define_syscall!(Listxattr, |path: CString, _list: *mut u8, _size: usize| {
    let path_str = path_from_raw(path)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, false)?;
    Ok(0)
});

define_syscall!(Llistxattr, |path: CString, _list: *mut u8, _size: usize| {
    let path_str = path_from_raw(path)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, true)?;
    Ok(0)
});

define_syscall!(Flistxattr, |object: ObjectRef,
                             _list: *mut u8,
                             _size: usize| {
    ensure_object_supports_xattrs(&object)?;
    Ok(0)
});

define_syscall!(Removexattr, |path: CString, name: CString| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, false)?;
    Err(SyscallError::NoData)
});

define_syscall!(Lremovexattr, |path: CString, name: CString| {
    let path_str = path_from_raw(path)?;
    let _name = xattr_name_from_raw(name)?;
    ensure_path_exists_at(AT_FDCWD, &path_str, true)?;
    Err(SyscallError::NoData)
});

define_syscall!(Fremovexattr, |object: ObjectRef, name: CString| {
    let _name = xattr_name_from_raw(name)?;
    ensure_object_supports_xattrs(&object)?;
    Err(SyscallError::NoData)
});

define_syscall!(UnlinkAt, |dirfd: i32, path: CString, flags: AtFlags| {
    let path = path_from_raw(path)?;
    if flags.bits() & !AtFlags::REMOVEDIR.bits() != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let path = resolve_path_at(dirfd, &path)?;
    let is_directory = matches!(
        VirtualFS.lock().file_info(path.clone())?.file_like_type,
        FileLikeType::Directory
    );
    if flags.contains(AtFlags::REMOVEDIR) {
        if !is_directory {
            return Err(SyscallError::NotADirectory);
        }
    } else if is_directory {
        return Err(SyscallError::IsADirectory);
    }
    VirtualFS.lock().delete_file(path)?;
    Ok(0)
});

define_syscall!(LinkAt, |old_dirfd: i32,
                         old_path: CString,
                         new_dirfd: i32,
                         new_path: CString,
                         _flags: i32| {
    let _ = path_is_relative_to_cwd(old_dirfd)?;
    let _ = path_is_relative_to_cwd(new_dirfd)?;
    let old_path = path_from_raw(old_path)?;
    let new_path = path_from_raw(new_path)?;
    let old_path = Path::new(&old_path);
    let new_path = Path::new(&new_path);

    VirtualFS.lock().link_file(old_path, new_path)?;

    Ok(0)
});

define_syscall!(SymlinkAt, |target: CString,
                            new_dirfd: i32,
                            link_path: CString| {
    let target = path_from_raw(target)?;
    let link_path = path_from_raw(link_path)?;
    let link_path = resolve_path_at(new_dirfd, &link_path)?;

    let result = VirtualFS.lock().create_symlink(link_path.clone(), &target);
    if result.is_err() && symlink_target_matches(&link_path, &target) {
        return Ok(0);
    }
    result?;

    Ok(0)
});

define_syscall!(MkdirAt, |dirfd: i32, path: CString, _mode: u32| {
    let path = path_from_raw(path)?;
    let path = match resolve_path_at(dirfd, &path) {
        Ok(path) => path,
        Err(err) => {
            if current_process_is_journald() {
                crate::s_println!(
                    "journald mkdirat resolve error: dirfd={} path={} err={:?}",
                    dirfd,
                    path,
                    err
                );
            }
            return Err(err);
        }
    };

    if let Err(err) = VirtualFS.lock().create_dir(path.clone()) {
        if current_process_is_journald() {
            crate::s_println!(
                "journald mkdirat error: path={} err={:?}",
                path.as_string(),
                err
            );
        }
        return Err(err.into());
    }

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
            VirtualFS.lock().open(path)?.chmod(mode)?;
            Ok(0)
        }
        _ => Err(SyscallError::NoSyscall),
    }
});

define_syscall!(Mkdir, |path: CString, mode: u32| {
    let _ = mode;
    let path = path_from_raw(path)?;
    let mut current_dir = with_current_process(|process| process.current_directory.clone());
    current_dir.push_path_str(&path);

    VirtualFS.lock().create_dir(current_dir.as_normal())?;
    Ok(0)
});

define_syscall!(Rmdir, |path: CString| {
    let path = path_from_raw(path)?;
    let mut current_dir = with_current_process(|process| process.current_directory.clone());
    current_dir.push_path_str(&path);
    let path = current_dir.as_normal();

    let is_directory = matches!(
        VirtualFS.lock().file_info(path.clone())?.file_like_type,
        FileLikeType::Directory
    );
    if !is_directory {
        return Err(SyscallError::NotADirectory);
    }

    VirtualFS.lock().delete_file(path)?;
    Ok(0)
});

define_syscall!(Mount, |source: CString,
                        target: CString,
                        filesystemtype: CString,
                        mountflags: u64,
                        data: CString| {
    let source = string_from_raw_optional(source)?;
    let target = path_from_raw(target)?;
    let filesystemtype = string_from_raw_optional(filesystemtype)?;
    let _data = string_from_raw_optional(data)?;
    if source.as_deref().is_some_and(should_trace_namespace_path)
        || should_trace_namespace_path(&target)
    {
        crate::s_println!(
            "mount trace source={:?} target={} fstype={:?} flags={:#x}",
            source,
            target,
            filesystemtype,
            mountflags
        );
    }
    let target_object = VirtualFS.lock().open(Path::new(&target))?;
    let target_path = target_object.path();
    let target_is_directory = matches!(
        target_object.info()?.file_like_type,
        FileLikeType::Directory
    );
    let operation_flags = MountOperationFlags::from_bits_retain(mountflags);

    if operation_flags.contains(MountOperationFlags::MS_BIND) {
        if operation_flags.contains(MountOperationFlags::MS_REMOUNT) {
            let (remount_flags, remount_mask) = remount_bind_flag_update(mountflags);
            VirtualFS
                .lock()
                .remount_bind(target_path, remount_flags, remount_mask)
                .map_err(SyscallError::from)?;
        } else {
            let source = source.ok_or(SyscallError::BadAddress)?;
            let source_path = resolve_path_at(AT_FDCWD, &source)?;
            VirtualFS
                .lock()
                .bind_mount(
                    source_path,
                    target_path,
                    operation_flags.contains(MountOperationFlags::MS_REC),
                )
                .map_err(SyscallError::from)?;
        }
        return Ok(0);
    }

    if operation_flags.contains(MountOperationFlags::MS_MOVE) {
        return Err(SyscallError::OperationNotSupported);
    }

    if (operation_flags.contains(MountOperationFlags::MS_REMOUNT)
        || operation_flags.intersects(
            MountOperationFlags::MS_PRIVATE
                | MountOperationFlags::MS_SLAVE
                | MountOperationFlags::MS_SHARED
                | MountOperationFlags::MS_UNBINDABLE,
        )
        || mountflags == 0
        || (mountflags & MountFlags::all().bits()) != 0)
        && filesystemtype.is_none()
    {
        return Ok(0);
    }

    if filesystemtype
        .as_deref()
        .is_some_and(|filesystemtype| !is_supported_api_mount(filesystemtype))
    {
        return Err(SyscallError::NoSyscall);
    }

    if filesystemtype.as_deref() == Some("tmpfs") {
        if !target_is_directory {
            return Err(SyscallError::NotADirectory);
        }
        VirtualFS.lock().resolve_dir(target_path.clone())?;
        VirtualFS
            .lock()
            .mount(target_path.clone(), TmpFs::new())
            .map_err(SyscallError::from)?;
    }
    Ok(0)
});

define_syscall!(Umount2, |target: CString, flags: UmountFlags| {
    let target = path_from_raw(target)?;
    let flags = validate_umount_flags(flags)?;
    let path = resolve_path_at(AT_FDCWD, &target)?.normalize();

    if flags.contains(UmountFlags::NOFOLLOW) {
        let _ = VirtualFS.lock().open_nofollow(path.clone())?;
    } else {
        let _ = VirtualFS.lock().open(path.clone())?;
    }

    if path == Path::new("/") {
        return Err(SyscallError::DeviceOrResourceBusy);
    }

    let mount_path = VirtualFS
        .lock()
        .mount_path(path.clone())
        .map_err(SyscallError::from)?;
    if mount_path != path {
        return Err(SyscallError::InvalidArguments);
    }

    if flags.contains(UmountFlags::DETACH) {
        VirtualFS
            .lock()
            .detach_mount(path)
            .map_err(SyscallError::from)?;
    } else {
        VirtualFS.lock().unmount(path).map_err(SyscallError::from)?;
    }
    Ok(0)
});

define_syscall!(Fsopen, |fs_name: CString, flags: FsOpenFlags| {
    let fs_name = path_from_raw(fs_name)?;
    let fd_flags = if flags.contains(FsOpenFlags::FSCONTEXT_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(FsContextObject::new(fs_name), fd_flags);
    Ok(fd)
});

define_syscall!(Fsconfig, |fd: i32,
                           cmd: u32,
                           key: CString,
                           value: CString,
                           _aux: i32| {
    let object = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    let fs_context = object.as_fs_context()?;
    let command = FsConfigCommand::try_from(cmd).map_err(|_| SyscallError::InvalidArguments)?;
    let key = string_from_raw_optional(key)?;
    let value = string_from_raw_optional(value)?;
    fs_context.configure(command, key.as_deref(), value.as_deref())?;
    Ok(0)
});

define_syscall!(Fsmount, |fd: i32,
                          flags: FsMountFlags,
                          _mount_attrs: u32| {
    let object = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    let fs_context = object.as_fs_context()?;
    let mount_path = next_api_mount_path()?;
    let mounted_fs = fs_context.created_fs()?;
    VirtualFS
        .lock()
        .mount_ref(mount_path.clone(), mounted_fs)
        .map_err(SyscallError::from)?;
    if let Some(mode) = fs_context.root_mode()? {
        VirtualFS.lock().open(mount_path.clone())?.chmod(mode)?;
    }

    let mount_root: ObjectRef = Arc::new(VirtualFS.lock().open(mount_path)?);
    let fd_flags = if flags.contains(FsMountFlags::FSMOUNT_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    Ok(get_current_process()
        .lock()
        .push_object_with_flags(mount_root, fd_flags))
});

define_syscall!(
    MoveMount,
    |from_dirfd: i32,
     from_path: CString,
     to_dirfd: i32,
     to_path: CString,
     flags: MoveMountFlags| {
        let source_path = if from_path.is_null() {
            if !flags.contains(MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH) {
                return Err(SyscallError::BadAddress);
            }
            let object =
                get_object_current_process(from_dirfd as u64).map_err(SyscallError::from)?;
            object.as_file_like()?.path().normalize()
        } else {
            let from_path = path_from_raw(from_path)?;
            if from_path.is_empty() {
                if !flags.contains(MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH) {
                    return Err(SyscallError::InvalidArguments);
                }
                let object =
                    get_object_current_process(from_dirfd as u64).map_err(SyscallError::from)?;
                object.as_file_like()?.path().normalize()
            } else {
                resolve_path_at(from_dirfd, &from_path)?.normalize()
            }
        };

        let (mount_path, mount_fs, mount_source_path, mount_flags) =
            VirtualFS.lock().mount_metadata(source_path.clone())?;
        if mount_path != source_path {
            return Err(SyscallError::InvalidArguments);
        }

        let target_path = if to_path.is_null() {
            if !flags.contains(MoveMountFlags::MOVE_MOUNT_T_EMPTY_PATH) {
                return Err(SyscallError::BadAddress);
            }
            let object = get_object_current_process(to_dirfd as u64).map_err(SyscallError::from)?;
            object.as_file_like()?.path().normalize()
        } else {
            let to_path = path_from_raw(to_path)?;
            if to_path.is_empty() {
                if !flags.contains(MoveMountFlags::MOVE_MOUNT_T_EMPTY_PATH) {
                    return Err(SyscallError::InvalidArguments);
                }
                let object =
                    get_object_current_process(to_dirfd as u64).map_err(SyscallError::from)?;
                object.as_file_like()?.path().normalize()
            } else {
                resolve_path_at(to_dirfd, &to_path)?.normalize()
            }
        };

        let _ = VirtualFS.lock().open(target_path.clone())?;

        VirtualFS
            .lock()
            .attach_mount(target_path, mount_fs, mount_source_path, mount_flags)
            .map_err(SyscallError::from)?;
        VirtualFS
            .lock()
            .unmount(source_path.clone())
            .map_err(SyscallError::from)?;
        if is_api_mount_path(&source_path) {
            let _ = VirtualFS.lock().delete_file(source_path);
        }

        let _ = flags.contains(MoveMountFlags::MOVE_MOUNT_BENEATH);
        Ok(0)
    }
);

define_syscall!(OpenTree, |dirfd: i32,
                           path: CString,
                           flags: OpenTreeFlags| {
    let object = if path.is_null() {
        if !flags.contains(OpenTreeFlags::AT_EMPTY_PATH) {
            return Err(SyscallError::BadAddress);
        }
        get_object_current_process(dirfd as u64).map_err(SyscallError::from)?
    } else {
        let path = path_from_raw(path)?;
        if should_trace_namespace_path(&path) || current_process_is_executor() {
            crate::s_println!(
                "open_tree trace dirfd={} path={} flags={:#x}",
                dirfd,
                path,
                flags.bits()
            );
        }
        if path.is_empty() && flags.contains(OpenTreeFlags::AT_EMPTY_PATH) {
            get_object_current_process(dirfd as u64).map_err(SyscallError::from)?
        } else {
            let path = resolve_path_at(dirfd, &path)?;
            if should_trace_namespace_path(&path.clone().as_string()) {
                crate::s_println!("open_tree resolved {}", path.clone().as_string());
            }
            let file = if flags.contains(OpenTreeFlags::AT_SYMLINK_NOFOLLOW) {
                VirtualFS.lock().open_nofollow(path)?
            } else {
                VirtualFS.lock().open(path)?
            };
            Arc::new(file)
        }
    };

    let _ = object.clone().as_file_like()?;

    let fd_flags = if flags.contains(OpenTreeFlags::OPEN_TREE_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let process = get_current_process();
    let fd = process.lock().push_object_with_flags(object, fd_flags);
    Ok(fd)
});

define_syscall!(MountSetattr, |dirfd: i32,
                               path: CString,
                               flags: AtFlags,
                               attr: *const LinuxMountAttr,
                               size: usize| {
    let allowed_flags =
        (AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH | AtFlags::RECURSIVE).bits();
    if flags.bits() & !allowed_flags != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if size < core::mem::size_of::<LinuxMountAttr>() {
        return Err(SyscallError::InvalidArguments);
    }
    if attr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let object = if path.is_null() {
        if !flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::BadAddress);
        }
        get_object_current_process(dirfd as u64).map_err(SyscallError::from)?
    } else {
        let path = path_from_raw(path)?;
        if should_trace_namespace_path(&path) || current_process_is_executor() {
            crate::s_println!(
                "mount_setattr trace dirfd={} path={} flags={:#x}",
                dirfd,
                path,
                flags.bits()
            );
        }
        if path.is_empty() {
            if !flags.contains(AtFlags::EMPTY_PATH) {
                return Err(SyscallError::BadAddress);
            }
            get_object_current_process(dirfd as u64).map_err(SyscallError::from)?
        } else {
            let path = resolve_path_at(dirfd, &path)?;
            if should_trace_namespace_path(&path.clone().as_string()) {
                crate::s_println!("mount_setattr resolved {}", path.clone().as_string());
            }
            let file = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
                VirtualFS.lock().open_nofollow(path)?
            } else {
                VirtualFS.lock().open(path)?
            };
            Arc::new(file)
        }
    };

    let _ = object.clone().as_file_like()?;

    let attr = unsafe { &*attr };
    if attr.attr_set != 0 || attr.attr_clr != 0 || attr.propagation != 0 || attr.userns_fd != 0 {
        return Err(SyscallError::OperationNotSupported);
    }

    Ok(0)
});

define_syscall!(
    NameToHandleAt,
    |dirfd: i32, path: CString, _handle: *mut LinuxFileHandle, _mount_id: *mut i32, flags: i32| {
        if flags != 0 {
            return Err(SyscallError::InvalidArguments);
        }

        let path = path_from_raw(path)?;
        ensure_path_exists_at(dirfd, &path, false)?;
        Err(SyscallError::OperationNotSupported)
    }
);

define_syscall!(Statfs, |path: CString, buf: *mut LinuxStatFs| {
    let path = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path)?;

    let _ = VirtualFS.lock().open(path.clone())?;
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
    if current_process_is_executor() {
        crate::s_println!("readlink trace path={}", path_str);
    }
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    readlink_impl(path, out_buf, out_len)
});

define_syscall!(ReadlinkAt, |dirfd: i32,
                             path: CString,
                             out_buf: *mut u8,
                             out_len: usize| {
    let path_str = path_from_raw(path)?;
    if current_process_is_executor() {
        crate::s_println!("readlinkat trace dirfd={} path={}", dirfd, path_str);
    }
    let path = resolve_path_at(dirfd, &path_str)?;
    readlink_impl(path, out_buf, out_len)
});

define_syscall!(RenameAt, |old_dirfd: i32,
                           old_path: CString,
                           new_dirfd: i32,
                           new_path: CString| {
    let old_from_currentdir = path_is_relative_to_cwd(old_dirfd)?;
    let new_from_currentdir = path_is_relative_to_cwd(new_dirfd)?;
    let old_path = path_from_raw(old_path)?;
    let new_path = path_from_raw(new_path)?;
    match rename_impl(
        old_from_currentdir,
        old_path.clone(),
        new_from_currentdir,
        new_path.clone(),
    ) {
        Ok(value) => Ok(value),
        Err(err) => {
            if current_process_is_journald() {
                crate::s_println!(
                    "journald renameat error: old_path={} new_path={} err={:?}",
                    old_path,
                    new_path,
                    err
                );
            }
            Err(err)
        }
    }
});

define_syscall!(RenameAt2, |old_dirfd: i32,
                            old_path: CString,
                            new_dirfd: i32,
                            new_path: CString,
                            flags: u32| {
    if flags != 0 {
        return Err(SyscallError::NoSyscall);
    }
    let old_from_currentdir = path_is_relative_to_cwd(old_dirfd)?;
    let new_from_currentdir = path_is_relative_to_cwd(new_dirfd)?;
    let old_path = path_from_raw(old_path)?;
    let new_path = path_from_raw(new_path)?;
    rename_impl(old_from_currentdir, old_path, new_from_currentdir, new_path)
});

define_syscall!(Utimensat, |dirfd: i32,
                            path: CString,
                            times: *const [i64; 2],
                            _flags: i32| {
    if !path.is_null() {
        let path_str = path_from_raw(path)?;
        let _ = resolve_path_at(dirfd, &path_str)?;
    }

    if !times.is_null() {
        unsafe {
            if (*times)[1] != UTIME_OMIT && (*times)[1] < 0 {
                return Err(SyscallError::InvalidArguments);
            }
        }
    }

    Ok(0)
});
