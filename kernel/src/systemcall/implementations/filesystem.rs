use crate::{
    define_syscall,
    filesystem::{
        absolute_path::AbsolutePath,
        errors::FSError,
        fusefs::FuseFs,
        info::{DirectoryContentInfo, FileLikeInfo, LinuxStat},
        object::{FileLikeObject, mount_device_id_for_path},
        path::Path,
        tmpfs::TmpFs,
        vfs::VirtualFS,
        vfs_operations::{
            file_info_path, open_path, open_path_nofollow, resolve_dir_path,
            resolve_path_with_mount_info,
        },
        vfs_traits::{DirectoryContentType, FileLikeType, MountFlags},
    },
    memory::user_safe,
    misc::{
        c_types::CString,
        others::KernelFrom,
        profile::{self, HotSyscallPhase},
    },
    object::{
        FileFlags,
        error::ObjectError,
        fs_context::{FsConfigCommand, FsContextObject},
        misc::{ObjectRef, get_object_current_process},
        traits::Statable,
    },
    process::{FdFlags, manager::get_current_process},
    systemcall::utils::{SyscallError, SyscallImpl},
};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;
use core::{
    mem::size_of,
    sync::atomic::{AtomicU64, Ordering},
};

const AT_FDCWD: i32 = -100;
const UTIME_NOW: i64 = 0x3fff_fffe;
const UTIME_OMIT: i64 = 0x3fff_ffff;
const STATX_BASIC_STATS: u32 = 0x0000_07ff;
const STATX_MNT_ID: u32 = 0x0000_1000;
const STATX_ATTR_MOUNT_ROOT: u64 = 0x0000_2000;
const ANON_INODE_FS_MAGIC: i64 = 0x0904_1934;
const SOCKFS_MAGIC: i64 = 0x534f_434b;
const AT_STATX_FORCE_SYNC: i32 = 0x2000;
const AT_STATX_DONT_SYNC: i32 = 0x4000;
const S_IFMT: u32 = 0o170000;
const SEELE_FILE_HANDLE_TYPE_INODE: i32 = 1;
const S_IFREG: u32 = 0o100000;
const S_IFIFO: u32 = 0o010000;
const S_IFCHR: u32 = 0o020000;
const S_IFBLK: u32 = 0o060000;
const S_IFSOCK: u32 = 0o140000;
const API_MOUNT_ROOT: &str = "/run/.api-mounts";
const MOUNT_ATTR_RDONLY: u64 = MountFlags::MS_RDONLY.bits();
const MOUNT_ATTR_NOSUID: u64 = MountFlags::MS_NOSUID.bits();
const MOUNT_ATTR_NODEV: u64 = MountFlags::MS_NODEV.bits();
const MOUNT_ATTR_NOEXEC: u64 = MountFlags::MS_NOEXEC.bits();
const MOUNT_ATTR__ATIME: u64 = 0x0000_0070;
const MOUNT_ATTR_NOATIME: u64 = 0x0000_0010;
const MOUNT_ATTR_STRICTATIME: u64 = 0x0000_0020;
const MOUNT_ATTR_NODIRATIME: u64 = 0x0000_0080;
const MOUNT_ATTR_IDMAP: u64 = 0x0010_0000;
const MOUNT_ATTR_NOSYMFOLLOW: u64 = 0x0020_0000;

static NEXT_API_MOUNT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TMPFILE_ID: AtomicU64 = AtomicU64::new(1);

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
#[derive(Clone, Copy)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
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

#[derive(Clone, Copy, Debug, Default)]
struct FuseMountOptions {
    fd: Option<u64>,
}

#[derive(Clone)]
struct PathLookup {
    stat: LinuxStat,
    mount_id: u64,
    mount_root: bool,
}

#[derive(Clone, Copy)]
struct PathLookupPhases {
    resolve: HotSyscallPhase,
    empty_path: HotSyscallPhase,
    resolve_final: HotSyscallPhase,
    build_stat: HotSyscallPhase,
    mount_info: HotSyscallPhase,
}

fn parse_fuse_mount_options(data: Option<&str>) -> Result<FuseMountOptions, SyscallError> {
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

fn resolve_path_at(dirfd: i32, path_str: &str) -> Result<Path, SyscallError> {
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
    let file_like = object.as_file_like()?;
    if !matches!(file_like.info()?.file_like_type, FileLikeType::Directory) {
        return Err(SyscallError::NotADirectory);
    }

    let base_path = file_like.path();
    let base = AbsolutePath::from_root_path(&base_path);
    let mut base = AbsolutePath::join_under_root(&base, &base, &Path::new("."));
    base.push_path_str(path_str);
    Ok(base.as_normal())
}

fn ensure_directory_exists(path: &str) -> Result<(), SyscallError> {
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

fn next_api_mount_path() -> Result<Path, SyscallError> {
    ensure_directory_exists(API_MOUNT_ROOT)?;
    let mount_id = NEXT_API_MOUNT_ID.fetch_add(1, Ordering::Relaxed);
    let path = Path::new(&format!("{API_MOUNT_ROOT}/{mount_id}"));
    ensure_directory_exists(&path.clone().as_string())?;
    Ok(path)
}

fn next_tmpfile_path(dir_path: &Path) -> Path {
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

fn open_tmpfile_at(dirfd: i32, path_str: &str) -> Result<ObjectRef, SyscallError> {
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

fn proc_self_fd_object(path: &Path) -> Result<Option<ObjectRef>, SyscallError> {
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

fn linux_stat_from_file_like_info(info: FileLikeInfo, path: &Path) -> LinuxStat {
    let rdev = info.rdev;
    let mut stat = info.as_linux();
    stat.st_dev = mount_device_id_for_path(path);
    stat.st_rdev = rdev;
    stat
}

fn mount_info_from_object(object: &ObjectRef) -> Result<(u64, bool), SyscallError> {
    Ok((mount_id_for_object(object)?, mount_root_for_object(object)?))
}

fn lookup_path_metadata(
    dirfd: i32,
    path_str: &str,
    nofollow: bool,
    allow_empty_path: bool,
    phases: PathLookupPhases,
) -> Result<PathLookup, SyscallError> {
    if path_str.is_empty() && allow_empty_path {
        let empty_path_start = profile::scope_start();
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        profile::record_hot_syscall_phase(
            phases.empty_path,
            profile::scope_start().saturating_sub(empty_path_start),
        );

        let build_stat_start = profile::scope_start();
        let stat = object.clone().as_statable()?.stat();
        profile::record_hot_syscall_phase(
            phases.build_stat,
            profile::scope_start().saturating_sub(build_stat_start),
        );

        let mount_info_start = profile::scope_start();
        let (mount_id, mount_root) = mount_info_from_object(&object)?;
        profile::record_hot_syscall_phase(
            phases.mount_info,
            profile::scope_start().saturating_sub(mount_info_start),
        );
        return Ok(PathLookup {
            stat,
            mount_id,
            mount_root,
        });
    }

    let resolve_start = profile::scope_start();
    let normalized_path = resolve_path_at(dirfd, path_str)?.normalize();
    profile::record_hot_syscall_phase(
        phases.resolve,
        profile::scope_start().saturating_sub(resolve_start),
    );

    let resolve_final_start = profile::scope_start();
    let (info, resolved_path, mount_id, mount_root) =
        resolve_path_with_mount_info(normalized_path, !nofollow)?;
    profile::record_hot_syscall_phase(
        phases.resolve_final,
        profile::scope_start().saturating_sub(resolve_final_start),
    );

    let build_stat_start = profile::scope_start();
    let stat = linux_stat_from_file_like_info(info, &resolved_path);
    profile::record_hot_syscall_phase(
        phases.build_stat,
        profile::scope_start().saturating_sub(build_stat_start),
    );

    Ok(PathLookup {
        stat,
        mount_id,
        mount_root,
    })
}

fn create_file_unlocked(path: Path) -> Result<(), SyscallError> {
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

fn profile_mkdir_common(dirfd: i32, path: &str, mode: u32) -> Result<(), SyscallError> {
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

fn is_api_mount_path(path: &Path) -> bool {
    path.clone()
        .as_string()
        .starts_with(&(String::from(API_MOUNT_ROOT) + "/"))
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

fn mount_attr_flag_update(attr: &LinuxMountAttr) -> Result<(MountFlags, MountFlags), SyscallError> {
    let supported_basic =
        MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV | MOUNT_ATTR_NOEXEC;
    let supported_set = supported_basic | MOUNT_ATTR_NOATIME | MOUNT_ATTR_STRICTATIME;
    let supported_clr = supported_basic | MOUNT_ATTR__ATIME;

    if attr.propagation != 0 {
        return Err(SyscallError::OperationNotSupported);
    }
    if attr.attr_set & !supported_set != 0 {
        return Err(SyscallError::OperationNotSupported);
    }
    if attr.attr_clr & !supported_clr != 0 {
        return Err(SyscallError::OperationNotSupported);
    }
    if attr.attr_set & (MOUNT_ATTR_NODIRATIME | MOUNT_ATTR_IDMAP | MOUNT_ATTR_NOSYMFOLLOW) != 0 {
        return Err(SyscallError::OperationNotSupported);
    }
    if (attr.attr_set & MOUNT_ATTR__ATIME) != 0 {
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
        mask.insert(MountFlags::MS_RELATIME);
    }

    Ok((flags, mask))
}

fn tmpfs_root_mode_from_mount_data(data: Option<&str>) -> Result<Option<u32>, SyscallError> {
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

fn mount_setattr_target_path(
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

fn check_access_mode(mode: i32) -> Result<(), SyscallError> {
    if (mode & !7) != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(())
}

fn check_access_permissions(stat: &LinuxStat, mode: i32) -> Result<(), SyscallError> {
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

    if object.clone().as_inet_socket().is_ok()
        || object.clone().as_netlink_socket().is_ok()
        || object.clone().as_unix_socket().is_ok()
    {
        return Ok(SOCKFS_MAGIC);
    }

    Err(SyscallError::BadFileDescriptor)
}

fn filesystem_magic_for_path(path: &Path) -> Result<i64, SyscallError> {
    let fs = {
        let (_mount_path, fs, _, _) = VirtualFS.lock().mount_metadata(path.clone())?;
        fs
    };
    Ok(fs.lock().magic())
}

fn mount_id_for_file_like(file_like: &FileLikeObject) -> Result<u64, SyscallError> {
    Ok(file_like.mount_id())
}

fn pseudo_mount_id(magic: i64) -> Option<u64> {
    let offset = match magic {
        SOCKFS_MAGIC => 0,
        ANON_INODE_FS_MAGIC => 1,
        _ => return None,
    };

    let mount_count = VirtualFS.lock().mount_count() as u64;
    Some(mount_count + 1 + offset)
}

fn mount_id_for_object(object: &ObjectRef) -> Result<u64, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return mount_id_for_file_like(&file_like);
    }

    let magic = filesystem_magic_for_object(object)?;
    pseudo_mount_id(magic).ok_or(SyscallError::BadFileDescriptor)
}

fn mount_root_for_object(object: &ObjectRef) -> Result<bool, SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        return Ok(file_like.mount_root());
    }

    let magic = filesystem_magic_for_object(object)?;
    if pseudo_mount_id(magic).is_some() {
        return Ok(false);
    }

    Err(SyscallError::BadFileDescriptor)
}

fn stat_mount_id_at(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<u64, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return mount_id_for_object(&object);
    }

    let path = resolve_path_at(dirfd, path_str)?;
    Ok(VirtualFS.lock().mount_id(path)?)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxFileHandle {
    handle_bytes: u32,
    handle_type: i32,
    f_handle: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SeeleFileHandle {
    inode: u64,
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

fn xattr_name_from_raw(name: CString) -> Result<String, SyscallError> {
    path_from_raw(name)
}

fn ensure_path_exists_at(dirfd: i32, path_str: &str, nofollow: bool) -> Result<(), SyscallError> {
    let path = resolve_path_at(dirfd, path_str)?;
    let _ = if nofollow {
        open_path_nofollow(path)?
    } else {
        open_path(path)?
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
    let open_result = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path.clone())
    } else {
        open_path(path.clone())
    };
    let object: ObjectRef = Arc::new(open_result?);
    check_access_permissions(&object.as_statable()?.stat(), mode)?;
    Ok(0)
}

fn rename_impl(
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

    VirtualFS
        .lock()
        .rename_file(old_path.clone(), new_path.clone())
        .map_err(SyscallError::from)?;
    Ok(0)
}

fn stat_at(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<LinuxStat, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return Ok(object.as_statable()?.stat());
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let open_result = if flags.contains(AtFlags::SYMLINK_NOFOLLOW) {
        open_path_nofollow(path.clone())
    } else {
        open_path(path.clone())
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

fn chmod_fd_object(object: ObjectRef, mode: u32) -> Result<(), SyscallError> {
    if let Ok(file_like) = object.clone().as_file_like() {
        file_like.chmod(mode)?;
    } else {
        let _ = object.as_statable()?;
    }

    Ok(())
}

fn chown_at(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<usize, SyscallError> {
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

fn chown_fd_object(object: ObjectRef) -> Result<(), SyscallError> {
    if object.clone().as_file_like().is_err() {
        let _ = object.as_statable()?;
    }

    Ok(())
}

mod directory;
mod fsinfo;
mod mount;
mod open;
mod path_ops;
mod stat;
mod time;
mod xattr;

pub use directory::*;
pub use fsinfo::*;
pub use mount::*;
pub use open::*;
pub use path_ops::*;
pub use stat::*;
pub use time::*;
pub use xattr::*;
