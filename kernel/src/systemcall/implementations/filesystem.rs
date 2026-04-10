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
    misc::usercopy::copy_to_user,
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
    let process = get_current_process();
    let path_str = process.lock().current_directory.clone().as_string();
    let path_bytes = path_str.as_bytes();
    let path_len = path_bytes.len();

    if len > path_len {
        if !copy_to_user(buf_ptr, path_bytes) || !copy_to_user(unsafe { buf_ptr.add(path_len) }, &[0]) {
            return Err(SyscallError::BadAddress);
        }
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
    let result = smart_navigate(path_str, object, start_from_current_dir, use_object)
        .ok_or(SyscallError::FileNotFound)?;
    let stat = result.as_file_like()?.info()?.as_linux();
    let stat_bytes = unsafe {
        core::slice::from_raw_parts(
            (&stat as *const LinuxStat).cast::<u8>(),
            core::mem::size_of::<LinuxStat>(),
        )
    };

    if !copy_to_user(linux_stat_ptr.cast::<u8>(), stat_bytes) {
        return Err(SyscallError::BadAddress);
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

define_syscall!(CreateDirectory, |path: String, from_current_dir: bool| {
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

define_syscall!(ReadLink, |path_str: String,
                           start_from_current_dir: bool,
                           out_buf: *mut u8,
                           out_len: usize| {
    let path = smart_resolve_path(path_str, start_from_current_dir)
        .ok_or(SyscallError::InvalidArguments)?;
    let target = VirtualFS.lock().open(path)?.read_link()?;
    let bytes = target.as_bytes();
    let copied = core::cmp::min(bytes.len(), out_len);

    if !copy_to_user(out_buf, &bytes[..copied]) {
        return Err(SyscallError::BadAddress);
    }

    Ok(copied)
});
