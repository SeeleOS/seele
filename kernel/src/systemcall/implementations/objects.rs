use core::slice;

use alloc::{collections::btree_map::BTreeMap, string::String};
use seele_sys::permission::Permissions;
use spin::Mutex;

use crate::{
    define_syscall,
    filesystem::vfs_traits::DirectoryContentType,
    filesystem::vfs_traits::Whence,
    misc::c_types::CString,
    object::{
        config::ConfigurateRequest,
        control::control_object,
        device::get_device,
        misc::{ObjectRef, get_object_current_process},
    },
    process::{
        manager::get_current_process,
        misc::{ProcessID, with_current_process},
    },
    systemcall::utils::{SyscallError, SyscallImpl},
};

static DIR_OFFSETS: Mutex<BTreeMap<(ProcessID, u64), usize>> = Mutex::new(BTreeMap::new());

#[repr(C)]
struct LinuxIovec {
    iov_base: *const u8,
    iov_len: usize,
}

define_syscall!(
    Getdents,
    |object_index: u64, buf: *mut u8, len: usize| {
        let obj = get_object_current_process(object_index)?.as_file_like()?;
        let contents = obj.directory_contents()?;
        let current_pid = get_current_process().lock().pid;
        let mut offsets = DIR_OFFSETS.lock();
        let offset_entry = offsets.entry((current_pid, object_index)).or_insert(0usize);
        let mut bytes_written = 0;

        while *offset_entry < contents.len() {
            let info = &contents[*offset_entry];
            let name_bytes = info.name.as_bytes();
            let reclen = ((20 + name_bytes.len() + 7) & !7) as u16;
            if bytes_written + reclen as usize > len {
                break;
            }

            unsafe {
                let entry_ptr = buf.add(bytes_written);
                entry_ptr.cast::<u64>().write_unaligned(1);
                entry_ptr
                    .add(8)
                    .cast::<i64>()
                    .write_unaligned((*offset_entry as i64) + 1);
                entry_ptr.add(16).cast::<u16>().write_unaligned(reclen);
                let linux_type = match info.content_type {
                    DirectoryContentType::Directory => 4,
                    DirectoryContentType::File => 8,
                    _ => 0,
                };
                entry_ptr.add(18).write(linux_type);
                core::ptr::copy_nonoverlapping(
                    name_bytes.as_ptr(),
                    entry_ptr.add(19),
                    name_bytes.len(),
                );
                entry_ptr.add(19 + name_bytes.len()).write(0);
            }

            bytes_written += reclen as usize;
            *offset_entry += 1;
        }

        if *offset_entry >= contents.len() && bytes_written == 0 {
            offsets.remove(&(current_pid, object_index));
            return Ok(0);
        }

        Ok(bytes_written)
    }
);

define_syscall!(Read, |object: ObjectRef,
                             buf_ptr: *mut u8,
                             len: usize| {
    unsafe {
        Ok(object
            .as_readable()?
            .read(slice::from_raw_parts_mut(buf_ptr, len))?)
    }
});

define_syscall!(Write, |object: ObjectRef,
                              buf_ptr: *mut u8,
                              len: usize| {
    unsafe {
        Ok(object
            .as_writable()?
            .write(slice::from_raw_parts(buf_ptr, len))?)
    }
});

define_syscall!(Writev, |object: ObjectRef, iov_ptr: u64, iovcnt: i32| {
    if iovcnt < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let writable = object.as_writable()?;
    let mut written = 0usize;
    let iov_ptr = iov_ptr as *const LinuxIovec;
    if iovcnt > 0 && iov_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let iovs = unsafe { slice::from_raw_parts(iov_ptr, iovcnt as usize) };
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let buf = unsafe { slice::from_raw_parts(iov.iov_base, iov.iov_len) };
        let count = writable.write(buf)?;
        written += count;
        if count < iov.iov_len {
            break;
        }
    }

    Ok(written)
});

define_syscall!(Close, |object_num: usize| {
    let process_ref = get_current_process();
    let mut process = process_ref.lock();
    let current_pid = process.pid;
    let objects = &mut process.objects;

    if objects.len() > object_num {
        let object = objects[object_num].take();
        drop(object);
        DIR_OFFSETS.lock().remove(&(current_pid, object_num as u64));
        Ok(0)
    } else {
        Err(SyscallError::BadFileDescriptor)
    }
});

define_syscall!(
    Ioctl,
    |object: ObjectRef, request: u64, request_ptr: u64| {
        let res = object
            .as_configuratable()?
            .configure(ConfigurateRequest::new(request, request_ptr)?);

        res.map(|val| val as usize).map_err(Into::into)
    }
);

define_syscall!(Fcntl, |object: ObjectRef, command: u64, arg: u64| {
    control_object(object, command, arg)
});

define_syscall!(Flock, |_object: ObjectRef, _operation: i32| {
    Ok(0)
});

define_syscall!(Fsync, |_object: ObjectRef| {
    Ok(0)
});

define_syscall!(Fdatasync, |_object: ObjectRef| {
    Ok(0)
});

define_syscall!(Ftruncate, |_object: ObjectRef, _length: i64| {
    Ok(0)
});

define_syscall!(Fallocate, |_object: ObjectRef, _mode: i32, _offset: i64, _len: i64| {
    Ok(0)
});

define_syscall!(Dup, |object: ObjectRef| {
    get_current_process()
        .lock()
        .clone_object(object)
        .map_err(Into::into)
});

define_syscall!(Dup3, |source: ObjectRef, dest: usize| {
    get_current_process()
        .lock()
        .clone_object_to(source, dest)
        .map_err(Into::into)
});

define_syscall!(OpenDevice, |name: String| {
    with_current_process(|process| {
        let device = get_device(name)?;
        let slot = process.push_object(device);

        Ok(slot)
    })
});

define_syscall!(
    MmapObject,
    |object: ObjectRef, pages: u64, offset: u64, permissions: Permissions| {
        let object = object.as_mappable()?;
        let address = object.map(offset, pages, permissions)?;

        Ok(address.as_u64() as usize)
    }
);

define_syscall!(
    Lseek,
    |object: ObjectRef, offset: i64, seek_type: Whence| {
        object
            .as_seekable()?
            .seek(offset, seek_type)
            .map_err(Into::into)
    }
);

define_syscall!(Pread64, |object: ObjectRef, buf_ptr: *mut u8, len: usize, offset: i64| {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let file = object.as_file_like()?;
    unsafe { Ok(file.read_at(slice::from_raw_parts_mut(buf_ptr, len), offset as u64)?) }
});

define_syscall!(Pwrite64, |object: ObjectRef, buf_ptr: *const u8, len: usize, offset: i64| {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let seekable = object.clone().as_seekable()?;
    let writable = object.as_writable()?;
    let current = seekable.clone().seek(0, Whence::Current)? as i64;
    seekable.clone().seek(offset, Whence::Start)?;
    let written = unsafe { writable.write(slice::from_raw_parts(buf_ptr, len))? };
    let _ = seekable.seek(current, Whence::Start);
    Ok(written)
});
