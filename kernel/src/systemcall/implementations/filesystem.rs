use core::{arch::x86_64::_store_mask8, result, slice};

use alloc::{string::String, sync::Arc};
use seele_sys::permission::Permissions;

use crate::{
    define_syscall,
    filesystem::{
        info::LinuxStat,
        misc::{smart_navigate, smart_resolve_path},
        path::Path,
        vfs::VirtualFS,
    },
    memory::addrspace::mem_area::Data,
    object::misc::ObjectRef,
    process::{manager::get_current_process, misc::with_current_process},
    systemcall::utils::{SyscallError, SyscallImpl},
};

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
    unsafe {
        let result = smart_navigate(path_str, object, start_from_current_dir, use_object)
            .ok_or(SyscallError::FileNotFound)?;

        *linux_stat_ptr = result.as_file_like()?.info()?.as_linux();
    }

    Ok(0)
});

define_syscall!(DeleteFile, |path: String| {
    VirtualFS.lock().delete_file(Path::new(&path))?;
    Ok(0)
});

define_syscall!(LinkFile, |old_path: String, new_path: String| {
    let old_path = Path::new(&old_path);
    let new_path = Path::new(&new_path);

    VirtualFS.lock().link_file(old_path, new_path)?;

    Ok(0)
});
