use core::slice;

use alloc::{string::String, sync::Arc};
use seele_sys::permission::Permissions;

use crate::{
    define_syscall,
    filesystem::{info::LinuxStat, path::Path, vfs::VirtualFS},
    object::misc::ObjectRef,
    process::manager::get_current_process,
    systemcall::utils::SyscallImpl,
};
use seele_sys::errors::SyscallError;

define_syscall!(OpenFile, |path_str: String, create: bool| {
    let path = Path::new(path_str.as_str());
    let object;
    if let Ok(file) = VirtualFS.lock().open(path.clone()) {
        object = Arc::new(file);
    } else if create {
        VirtualFS.lock().create_file(path.clone())?;
        object = Arc::new(VirtualFS.lock().open(path)?);
    } else {
        return Err(SyscallError::FileNotFound);
    }

    let current_process = get_current_process();
    let slot = current_process.lock().alloc_object_slot();
    current_process.lock().objects[slot] = Some(object);
    Ok(slot)
});

define_syscall!(ChangeDirectory, |dir: String| {
    let path = Path::new(&dir).as_absolute();
    get_current_process().lock().change_directory(path)?;
    Ok(0)
});

define_syscall!(GetCurrentDirectory, |buf_ptr: *mut u8, len: usize| {
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

define_syscall!(FileInfo, |start_from_current_dir: bool,
                           path_str: String,
                           linux_stat_ptr: *mut LinuxStat,
                           use_object: bool,
                           object: ObjectRef| {
    let path: Path;
    if !use_object {
        if path_str.starts_with('/') {
            path = Path::new(&path_str);
        } else if start_from_current_dir {
            let mut cur_path = get_current_process().lock().current_directory.clone();
            cur_path.push_path_str(&path_str);
            path = cur_path.clone().as_normal();
        } else {
            return Err(SyscallError::other(
                "Non-absolute paths are not supported yet",
            ));
        }
    } else {
        unsafe { *linux_stat_ptr = object.as_file_like()?.info()?.as_linux() };
        return Ok(0);
    }

    let info = VirtualFS.lock().file_info(path)?;
    unsafe { *linux_stat_ptr = info.as_linux() };
    Ok(0)
});

define_syscall!(
    MapFile,
    |object: ObjectRef, len: u64, offset: u64, permissions: Permissions| {
        Ok(get_current_process()
            .lock()
            .addrspace
            .map_file(
                object.as_file_like()?,
                offset,
                len.div_ceil(4096),
                permissions,
            )
            .as_u64() as usize)
    }
);
