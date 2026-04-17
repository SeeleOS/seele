use core::slice;

use alloc::{string::String, sync::Arc};
use bitflags::bitflags;

use crate::{
    define_syscall,
    filesystem::{
        info::LinuxStat,
        misc::{smart_navigate, smart_resolve_path},
        path::Path,
        vfs::VirtualFS,
    },
    memory::addrspace::mem_area::Data,
    misc::{c_types::CString, others::KernelFrom},
    object::misc::{ObjectRef, get_object_current_process},
    process::{manager::get_current_process, misc::with_current_process},
    s_println,
    systemcall::utils::{SyscallError, SyscallImpl},
};

const AT_FDCWD: i32 = -100;
const UTIME_OMIT: i64 = 0x3fff_ffff;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct AtFlags: i32 {
        const REMOVEDIR = 0x200;
        const SYMLINK_NOFOLLOW = 0x100;
        const EMPTY_PATH = 0x1000;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct OpenFlags: i32 {
        const CREAT = 0x40;
        const EXCL = 0x80;
    }
}

fn device_from_path(path: &str) -> Option<&'static str> {
    match path {
        "/dev/fb0" => Some("framebuffer"),
        "/dev/null" => Some("devnull"),
        "/dev/tty" | "/dev/console" | "/dev/tty0" | "/dev/tty1" => Some("tty"),
        "/dev/psaux" | "/dev/mouse" => Some("ps2mouse"),
        _ => None,
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
    let target = VirtualFS.lock().open(path)?.read_link()?;
    let bytes = target.as_bytes();
    let copied = core::cmp::min(bytes.len(), out_len);

    unsafe {
        slice::from_raw_parts_mut(out_buf, copied).copy_from_slice(&bytes[..copied]);
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

    VirtualFS.lock().link_file(old_path.clone(), new_path)?;
    VirtualFS.lock().delete_file(old_path)?;

    Ok(0)
}

define_syscall!(OpenAt, |dirfd: i32,
                         path: CString,
                         flags: i32,
                         _mode: u32| {
    let current_process = get_current_process();
    let path_str = path_from_raw(path)?;
    let flags = OpenFlags::from_bits_truncate(flags);
    let create = flags.contains(OpenFlags::CREAT);

    let path = Path::new(path_str.as_str());
    let object;
    if let Ok(file) = VirtualFS.lock().open(path.clone()) {
        if create && flags.contains(OpenFlags::EXCL) {
            return Err(SyscallError::FileAlreadyExists);
        }
        object = Arc::new(file);
    } else if create {
        VirtualFS.lock().create_file(path.clone())?;
        object = Arc::new(VirtualFS.lock().open(path)?);
    } else if let Some(device) = device_from_path(&path_str) {
        let device = crate::object::device::get_device(device.into())
            .map_err(|_| SyscallError::FileNotFound)?;
        let slot = current_process.lock().push_object(device);
        return Ok(slot);
    } else {
        return Err(SyscallError::FileNotFound);
    }

    let _ = path_is_relative_to_cwd(dirfd)?;

    let slot = current_process.lock().alloc_object_slot();
    current_process.lock().objects[slot] = Some(object);
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

define_syscall!(Unlink, |path: CString| {
    UnlinkAt::handle_call(AT_FDCWD as u64, path as u64, 0, 0, 0, 0)
});

define_syscall!(Getcwd, |buf_ptr: *mut u8, len: usize| {
    if buf_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let buf = unsafe { slice::from_raw_parts_mut(buf_ptr, len) };
    let process = get_current_process();
    let path_str = process.lock().current_directory.clone().as_string();
    let path_bytes = path_str.as_bytes();
    let path_len = path_bytes.len();

    if len > path_len {
        buf[..path_len].copy_from_slice(path_bytes);
        buf[path_len] = 0;
    } else {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(path_len + 1)
});

define_syscall!(Fstat, |fd: u64, linux_stat_ptr: *mut LinuxStat| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    unsafe {
        *linux_stat_ptr = object.as_statable()?.stat();
    }
    Ok(0)
});

define_syscall!(Fchmod, |fd: u64, _mode: u32| {
    let _ = get_object_current_process(fd).map_err(SyscallError::from)?;
    Ok(0)
});

define_syscall!(Newfstatat, |dirfd: i32,
                             path: u64,
                             linux_stat_ptr: *mut LinuxStat,
                             flags: i32| {
    if linux_stat_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }
    let path = path as CString;
    if path.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let path_str = path_from_raw(path)?;
    let flags = AtFlags::from_bits_truncate(flags);
    if flags.bits() != flags.bits() & (AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH).bits() {
        return Err(SyscallError::NoSyscall);
    }

    let stat = if path_str.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
        let object = get_object_current_process(dirfd as u64).map_err(SyscallError::from)?;
        object.as_statable()?.stat()
    } else {
        let path = resolve_path_at(dirfd, &path_str)?;
        let object: ObjectRef = Arc::new(VirtualFS.lock().open(path)?);
        object.as_statable()?.stat()
    };

    unsafe {
        *linux_stat_ptr = stat;
    }
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
