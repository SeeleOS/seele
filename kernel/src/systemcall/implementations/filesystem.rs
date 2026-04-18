use crate::{
    define_syscall,
    filesystem::{
        errors::FSError,
        info::LinuxStat,
        misc::{smart_navigate, smart_resolve_path},
        path::Path,
        vfs::VirtualFS,
    },
    memory::{addrspace::mem_area::Data, user_safe},
    misc::{c_types::CString, others::KernelFrom},
    object::misc::{ObjectRef, get_object_current_process},
    process::{manager::get_current_process, misc::with_current_process},
    s_println,
    systemcall::utils::{SyscallError, SyscallImpl},
};
use alloc::{string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;

const AT_FDCWD: i32 = -100;
const UTIME_OMIT: i64 = 0x3fff_ffff;
const STATX_BASIC_STATS: u32 = 0x0000_07ff;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct AtFlags: i32 {
        const REMOVEDIR = 0x200;
        const SYMLINK_NOFOLLOW = 0x100;
        const EMPTY_PATH = 0x1000;
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

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct OpenFlags: i32 {
        const CREAT = 0x40;
        const EXCL = 0x80;
    }
}

fn path_from_raw(path: CString) -> Result<String, SyscallError> {
    String::k_from(path).map_err(|_| SyscallError::InvalidArguments)
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

    Err(SyscallError::NoSyscall)
}

fn check_access_mode(mode: i32) -> Result<(), SyscallError> {
    if (mode & !7) != 0 {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(())
}

fn readlink_impl(
    path_str: String,
    start_from_current_dir: bool,
    out_buf: *mut u8,
    out_len: usize,
) -> Result<usize, SyscallError> {
    let path = smart_resolve_path(path_str, start_from_current_dir)
        .ok_or(SyscallError::InvalidArguments)?;
    let target = match VirtualFS.lock().open(path)?.read_link() {
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

    match VirtualFS.lock().delete_file(new_path.clone()) {
        Ok(()) | Err(FSError::NotFound) => {}
        Err(err) => return Err(err.into()),
    }

    VirtualFS
        .lock()
        .link_file(old_path.clone(), new_path.clone())?;
    VirtualFS.lock().delete_file(old_path.clone())?;

    Ok(0)
}

fn stat_at(dirfd: i32, path_str: &str, flags: AtFlags) -> Result<LinuxStat, SyscallError> {
    if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        return Ok(object.as_statable()?.stat());
    }

    let path = resolve_path_at(dirfd, path_str)?;
    let object: ObjectRef = Arc::new(VirtualFS.lock().open(path)?);
    Ok(object.as_statable()?.stat())
}

define_syscall!(OpenAt, |dirfd: i32,
                         path: CString,
                         flags: i32,
                         _mode: u32| {
    let current_process = get_current_process();
    let path_str = path_from_raw(path)?;
    let flags = OpenFlags::from_bits_truncate(flags);
    let create = flags.contains(OpenFlags::CREAT);

    let path = resolve_path_at(dirfd, &path_str)?;
    let object;
    if let Ok(file) = VirtualFS.lock().open(path.clone()) {
        if create && flags.contains(OpenFlags::EXCL) {
            return Err(SyscallError::FileAlreadyExists);
        }
        object = Arc::new(file);
    } else if create {
        VirtualFS.lock().create_file(path.clone())?;
        object = Arc::new(VirtualFS.lock().open(path)?);
    } else {
        s_println!(
            "openat path={} flags={:#x} -> err {:?}",
            path_str,
            flags.bits(),
            SyscallError::FileNotFound
        );
        return Err(SyscallError::FileNotFound);
    }

    let slot = current_process.lock().alloc_object_slot();
    current_process.lock().objects[slot] = Some(object);
    s_println!(
        "openat path={} flags={:#x} -> fd {}",
        path_str,
        flags.bits(),
        slot
    );
    Ok(slot)
});

define_syscall!(Open, |path: CString, flags: i32, mode: u32| {
    OpenAt::handle_call(
        (-100i32) as u64,
        path as u64,
        flags as u64,
        mode as u64,
        0,
        0,
    )
});

define_syscall!(Access, |path: CString, mode: i32| {
    check_access_mode(mode)?;
    let path_str = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    let _ = VirtualFS.lock().open(path)?;
    Ok(0)
});

define_syscall!(Chdir, |dir: String| {
    let path = Path::new(&dir).as_absolute();
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

define_syscall!(Chmod, |path: CString, mode: u32| {
    let path_str = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path_str)?;
    let file = VirtualFS.lock().open(path)?;
    file.chmod(mode)?;
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
        user_safe::write(buf_ptr, &buffer)?;
    } else {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(path_len + 1)
});

define_syscall!(Fstat, |fd: u64, linux_stat_ptr: *mut LinuxStat| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    let stat = object.as_statable()?.stat();
    if stat.st_mode & 0o170000 == 0o040000 {
        s_println!(
            "fstat fd={} mode={:#o} uid={} gid={}",
            fd,
            stat.st_mode,
            stat.st_uid,
            stat.st_gid
        );
    }
    user_safe::write(linux_stat_ptr, &stat)?;
    Ok(0)
});

define_syscall!(Fchmod, |fd: u64, mode: u32| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    object.as_file_like()?.chmod(mode)?;
    Ok(0)
});

define_syscall!(Newfstatat, |dirfd: i32,
                             path: u64,
                             linux_stat_ptr: *mut LinuxStat,
                             flags: i32| {
    let path = path as CString;
    if path.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let path_str = path_from_raw(path)?;
    let flags = AtFlags::from_bits_truncate(flags);
    if flags.bits() != flags.bits() & (AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH).bits() {
        return Err(SyscallError::NoSyscall);
    }

    let stat = stat_at(dirfd, &path_str, flags)?;

    if path_str.contains("X11")
        || path_str.contains(".X")
        || path_str.contains("/tmp")
        || path_str.contains("local")
    {
        s_println!(
            "newfstatat dirfd={} path={} flags={:#x} mode={:#o} uid={} gid={}",
            dirfd,
            path_str,
            flags.bits(),
            stat.st_mode,
            stat.st_uid,
            stat.st_gid
        );
    }
    user_safe::write(linux_stat_ptr, &stat)?;
    Ok(0)
});

define_syscall!(Statx, |dirfd: i32,
                        path: CString,
                        flags: i32,
                        _mask: u32,
                        statx_ptr: u64| {
    let statx_ptr = statx_ptr as *mut LinuxStatx;
    let path_str = path_from_raw(path)?;
    let flags = AtFlags::from_bits_truncate(flags);
    if flags.bits() != flags.bits() & (AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH).bits() {
        return Err(SyscallError::NoSyscall);
    }

    let stat = stat_at(dirfd, &path_str, flags)?;

    let statx = LinuxStatx {
        stx_mask: STATX_BASIC_STATS,
        stx_blksize: stat.st_blksize as u32,
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
        ..Default::default()
    };
    user_safe::write(statx_ptr as *mut LinuxStatx, &statx)?;

    Ok(0)
});

define_syscall!(Faccessat, |dirfd: i32,
                            path: CString,
                            mode: i32,
                            _flags: i32| {
    check_access_mode(mode)?;
    let path_str = path_from_raw(path)?;
    let path = resolve_path_at(dirfd, &path_str)?;
    let _ = VirtualFS.lock().open(path)?;
    Ok(0)
});

define_syscall!(Faccessat2, |dirfd: i32,
                             path: CString,
                             mode: i32,
                             flags: i32| {
    Faccessat::handle_call(dirfd as u64, path as u64, mode as u64, flags as u64, 0, 0)
});

define_syscall!(UnlinkAt, |dirfd: i32, path: CString, flags: i32| {
    let _ = path_is_relative_to_cwd(dirfd)?;
    let path = path_from_raw(path)?;
    if AtFlags::from_bits_truncate(flags).contains(AtFlags::REMOVEDIR) {
        return Err(SyscallError::NoSyscall);
    }
    VirtualFS.lock().delete_file(Path::new(&path))?;
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

define_syscall!(MkdirAt, |dirfd: i32, path: CString, _mode: u32| {
    let path = path_from_raw(path)?;
    let from_current_dir = path_is_relative_to_cwd(dirfd)?;
    let path = match from_current_dir {
        true => {
            let mut current_dir = with_current_process(|process| process.current_directory.clone());

            current_dir.push_path_str(&path);

            current_dir.as_normal()
        }
        false => Path::new(&path),
    };

    VirtualFS.lock().create_dir(path)?;

    Ok(0)
});

define_syscall!(Mkdir, |path: CString, mode: u32| {
    let _ = mode;
    let path = path_from_raw(path)?;
    let mut current_dir = with_current_process(|process| process.current_directory.clone());
    current_dir.push_path_str(&path);

    VirtualFS.lock().create_dir(current_dir.as_normal())?;
    Ok(0)
});

define_syscall!(Statfs, |path: CString, buf: u64| {
    let buf = buf as *mut LinuxStatFs;
    let path = path_from_raw(path)?;
    let mut current_dir = with_current_process(|process| process.current_directory.clone());
    current_dir.push_path_str(&path);
    let path = current_dir.as_normal();

    let _ = VirtualFS.lock().open(path)?;

    let statfs = LinuxStatFs {
        f_type: 0xEF53,
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
    };
    user_safe::write(buf as *mut LinuxStatFs, &statfs)?;

    Ok(0)
});

define_syscall!(Readlink, |path: CString,
                           out_buf: *mut u8,
                           out_len: usize| {
    let path_str = path_from_raw(path)?;
    let start_from_current_dir = true;
    readlink_impl(path_str, start_from_current_dir, out_buf, out_len)
});

define_syscall!(ReadlinkAt, |dirfd: i32,
                             path: CString,
                             out_buf: *mut u8,
                             out_len: usize| {
    let path_str = path_from_raw(path)?;
    let start_from_current_dir = path_is_relative_to_cwd(dirfd)?;
    readlink_impl(path_str, start_from_current_dir, out_buf, out_len)
});

define_syscall!(RenameAt, |old_dirfd: i32,
                           old_path: CString,
                           new_dirfd: i32,
                           new_path: CString| {
    let old_from_currentdir = path_is_relative_to_cwd(old_dirfd)?;
    let new_from_currentdir = path_is_relative_to_cwd(new_dirfd)?;
    let old_path = path_from_raw(old_path)?;
    let new_path = path_from_raw(new_path)?;
    rename_impl(old_from_currentdir, old_path, new_from_currentdir, new_path)
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
                            path: u64,
                            times: u64,
                            _flags: i32| {
    let path = path as CString;
    if !path.is_null() {
        let path_str = path_from_raw(path)?;
        let _ = resolve_path_at(dirfd, &path_str)?;
    }

    if times != 0 {
        let times = times as *const [i64; 2];
        unsafe {
            if (*times)[1] != UTIME_OMIT && (*times)[1] < 0 {
                return Err(SyscallError::InvalidArguments);
            }
        }
    }

    Ok(0)
});
