use core::slice;

use alloc::{string::String, sync::Arc};
use seele_sys::abi::object::device_from_path;

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
const AT_REMOVEDIR: i32 = 0x200;
const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
const AT_EMPTY_PATH: i32 = 0x1000;

const O_CREAT: i32 = 0x40;
const O_EXCL: i32 = 0x80;

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

define_syscall!(OpenAt, |dirfd: i32, path: CString, flags: i32, _mode: u32| {
    let current_process = get_current_process();
    let path_str = path_from_raw(path)?;
    let create = (flags & O_CREAT) != 0;

    let path = Path::new(path_str.as_str());
    let object;
    if let Ok(file) = VirtualFS.lock().open(path.clone()) {
        if create && (flags & O_EXCL) != 0 {
            return Err(SyscallError::FileAlreadyExists);
        }
        object = Arc::new(file);
    } else if create {
        VirtualFS.lock().create_file(path.clone())?;
        object = Arc::new(VirtualFS.lock().open(path)?);
    } else if let Some(device) = device_from_path(&path_str) {
        let device = crate::object::device::get_device(
            String::k_from(device).map_err(|_| SyscallError::InvalidArguments)?,
        )
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

define_syscall!(Chdir, |dir: String| {
    let path = Path::new(&dir).as_absolute();
    get_current_process().lock().change_directory(path)?;
    Ok(0)
});

define_syscall!(Getcwd, |buf_ptr: *mut u8, len: usize| {
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

    Ok(buf_ptr as usize)
});

define_syscall!(Fstat, |fd: u64, linux_stat_ptr: *mut LinuxStat| {
    let object = get_object_current_process(fd).map_err(SyscallError::from)?;
    unsafe {
        *linux_stat_ptr = object.as_statable()?.stat();
    }
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
    let unsupported_flags = flags & !(AT_SYMLINK_NOFOLLOW | AT_EMPTY_PATH);
    if unsupported_flags != 0 {
        return Err(SyscallError::NoSyscall);
    }

    let stat = if path_str.is_empty() && (flags & AT_EMPTY_PATH) != 0 {
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

define_syscall!(UnlinkAt, |dirfd: i32, path: CString, flags: i32| {
    let _ = path_is_relative_to_cwd(dirfd)?;
    let path = path_from_raw(path)?;
    if flags == AT_REMOVEDIR {
        return Err(SyscallError::NoSyscall);
    }
    VirtualFS.lock().delete_file(Path::new(&path))?;
    Ok(0)
});

define_syscall!(LinkAt, |old_dirfd: i32, old_path: CString, new_dirfd: i32, new_path: CString, _flags: i32| {
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
    rename_impl(
        old_from_currentdir,
        old_path,
        new_from_currentdir,
        new_path,
    )
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
    rename_impl(
        old_from_currentdir,
        old_path,
        new_from_currentdir,
        new_path,
    )
});
