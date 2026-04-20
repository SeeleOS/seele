use core::{
    slice,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{collections::btree_map::BTreeMap, format, string::String};
use bitflags::bitflags;
use spin::Mutex;

use crate::{
    define_syscall,
    filesystem::path::Path,
    filesystem::vfs_traits::DirectoryContentType,
    filesystem::vfs_traits::Whence,
    memory::protection::Protection,
    object::{
        config::ConfigurateRequest,
        control::control_object,
        device::get_device,
        memfd::create_memfd_object,
        misc::{ObjectRef, get_object_current_process},
    },
    process::{
        FdFlags,
        manager::get_current_process,
        misc::{ProcessID, with_current_process},
    },
    s_println,
    systemcall::utils::{SyscallError, SyscallImpl, SyscallResult},
};

static DIR_OFFSETS: Mutex<BTreeMap<(ProcessID, u64), usize>> = Mutex::new(BTreeMap::new());
static MEMFD_COUNTER: AtomicU64 = AtomicU64::new(0);
const COPY_CHUNK_SIZE: usize = 16 * 1024;

fn current_process_is_executor() -> bool {
    with_current_process(|process| {
        process
            .command_line
            .first()
            .is_some_and(|path| path.ends_with("/systemd-executor"))
    })
}

fn trace_executor_mountinfo_object(object: &ObjectRef, action: &str) {
    if !current_process_is_executor() {
        return;
    }

    let Ok(file_like) = object.clone().as_file_like() else {
        return;
    };
    let path = file_like.path().as_string();
    if !path.ends_with("/mountinfo") {
        return;
    }

    s_println!("mountinfo object trace action={} path={}", action, path);
}

#[repr(C)]
struct LinuxIovec {
    iov_base: *const u8,
    iov_len: usize,
}

fn write_dirents64(object_index: u64, buf: *mut u8, len: usize) -> SyscallResult {
    let obj = get_object_current_process(object_index)?.as_file_like()?;
    let contents = match obj.directory_contents() {
        Ok(contents) => contents,
        Err(err) => {
            s_println!(
                "getdents64 failed fd={} path={} err={:?}",
                object_index,
                obj.path().as_string(),
                err
            );
            return Err(err.into());
        }
    };
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

fn copy_between_objects(
    input: ObjectRef,
    output: ObjectRef,
    mut remaining: usize,
) -> SyscallResult {
    let readable = input.as_readable()?;
    let writable = output.as_writable()?;
    let mut buffer = [0u8; COPY_CHUNK_SIZE];
    let mut total = 0usize;

    while remaining > 0 {
        let chunk_len = remaining.min(buffer.len());
        let read = readable.read(&mut buffer[..chunk_len])?;
        if read == 0 {
            break;
        }

        let mut written = 0usize;
        while written < read {
            let count = writable.write(&buffer[written..read])?;
            if count == 0 {
                return Err(SyscallError::BrokenPipe);
            }
            written += count;
        }

        total += read;
        remaining -= read;
        if read < chunk_len {
            break;
        }
    }

    Ok(total)
}

define_syscall!(Getdents, |object_index: u64, buf: *mut u8, len: usize| {
    write_dirents64(object_index, buf, len)
});

define_syscall!(Getdents64, |object_index: u64, buf: *mut u8, len: usize| {
    write_dirents64(object_index, buf, len)
});

define_syscall!(Read, |object: ObjectRef, buf_ptr: *mut u8, len: usize| {
    trace_executor_mountinfo_object(&object, "read-enter");
    unsafe {
        let read = object
            .clone()
            .as_readable()?
            .read(slice::from_raw_parts_mut(buf_ptr, len))?;
        trace_executor_mountinfo_object(&object, "read-exit");
        Ok(read)
    }
});

define_syscall!(Write, |object: ObjectRef, buf_ptr: *mut u8, len: usize| {
    unsafe {
        Ok(object
            .as_writable()?
            .write(slice::from_raw_parts(buf_ptr, len))?)
    }
});

define_syscall!(Writev, |object: ObjectRef,
                         iov_ptr: *const LinuxIovec,
                         iovcnt: i32| {
    if iovcnt < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let writable = object.as_writable()?;
    let mut written = 0usize;
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

define_syscall!(Sendfile, |out_fd: ObjectRef,
                           in_fd: ObjectRef,
                           offset: *mut i64,
                           count: usize| {
    if !offset.is_null() {
        return Err(SyscallError::OperationNotSupported);
    }

    copy_between_objects(in_fd, out_fd, count)
});

define_syscall!(CopyFileRange, |fd_in: ObjectRef,
                                off_in: *mut i64,
                                fd_out: ObjectRef,
                                off_out: *mut i64,
                                len: usize,
                                flags: u32| {
    if !off_in.is_null() || !off_out.is_null() {
        return Err(SyscallError::OperationNotSupported);
    }
    if flags != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    copy_between_objects(fd_in, fd_out, len)
});

define_syscall!(Close, |object_num: usize| {
    let process_ref = get_current_process();
    let mut process = process_ref.lock();
    let current_pid = process.pid;
    if process.clear_object_slot(object_num).is_ok() {
        DIR_OFFSETS.lock().remove(&(current_pid, object_num as u64));
        Ok(0)
    } else {
        Err(SyscallError::BadFileDescriptor)
    }
});

define_syscall!(Ioctl, |object: ObjectRef,
                        request: u64,
                        request_ptr: u64| {
    if request == 0x802c_542a {
        if current_process_is_executor() {
            if let Ok(file_like) = object.clone().as_file_like() {
                crate::s_println!(
                    "ioctl trace request={:#x} file_like_path={}",
                    request,
                    file_like.path().as_string()
                );
            } else {
                crate::s_println!(
                    "ioctl trace request={:#x} object={}",
                    request,
                    object.debug_name()
                );
            }
        }
    }
    let res = object
        .as_configuratable()?
        .configure(ConfigurateRequest::new(request, request_ptr)?);

    res.map(|val| val as usize).map_err(Into::into)
});

define_syscall!(Fcntl, |fd: u64, command: u64, arg: u64| {
    control_object(fd, command, arg)
});

define_syscall!(Flock, |_object: ObjectRef, _operation: i32| { Ok(0) });

define_syscall!(Fsync, |_object: ObjectRef| { Ok(0) });

define_syscall!(Fdatasync, |_object: ObjectRef| { Ok(0) });

define_syscall!(Fadvise64, |_object: ObjectRef,
                            _offset: i64,
                            _len: i64,
                            advice: i32| {
    if !(0..=5).contains(&advice) {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(0)
});

define_syscall!(Ftruncate, |_object: ObjectRef, _length: i64| { Ok(0) });

define_syscall!(Fallocate, |_object: ObjectRef,
                            _mode: i32,
                            _offset: i64,
                            _len: i64| { Ok(0) });

define_syscall!(Dup, |object: ObjectRef| {
    get_current_process()
        .lock()
        .clone_object(object)
        .map_err(Into::into)
});

define_syscall!(Dup2, |source_fd: usize, dest: usize| {
    if source_fd == dest {
        return Ok(dest);
    }

    let source = get_object_current_process(source_fd as u64).map_err(SyscallError::from)?;
    get_current_process()
        .lock()
        .clone_object_to(source, dest)
        .map_err(Into::into)
});

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct DupFlags: i32 {
        const O_CLOEXEC = 0o2_000_000;
    }
}

define_syscall!(Dup3, |source_fd: usize, dest: usize, flags: i32| {
    let flags = DupFlags::from_bits(flags).ok_or(SyscallError::InvalidArguments)?;
    if source_fd == dest {
        return Err(SyscallError::InvalidArguments);
    }

    let source = get_object_current_process(source_fd as u64).map_err(SyscallError::from)?;
    let fd_flags = if flags.contains(DupFlags::O_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    get_current_process()
        .lock()
        .clone_object_to_with_flags(source, dest, fd_flags)
        .map_err(Into::into)
});

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct CloseRangeFlags: u32 {
        const CLOSE_RANGE_CLOEXEC = 0x4;
    }
}

define_syscall!(CloseRange, |first: usize, last: usize, flags: u32| {
    let flags = CloseRangeFlags::from_bits(flags).ok_or(SyscallError::InvalidArguments)?;

    let process_ref = get_current_process();
    let mut process = process_ref.lock();
    if first >= process.objects.len() {
        return Ok(0);
    }

    let end = last.min(process.objects.len().saturating_sub(1));
    for fd in first..=end {
        if process.objects[fd].is_none() {
            continue;
        }
        if flags.contains(CloseRangeFlags::CLOSE_RANGE_CLOEXEC) {
            process.set_fd_flags(fd, FdFlags::CLOEXEC)?;
        } else {
            process.clear_object_slot(fd)?;
        }
    }

    Ok(0)
});

define_syscall!(OpenDevice, |name: String| {
    with_current_process(|process| {
        let device = get_device(name)?;
        let slot = process.push_object(device);

        Ok(slot)
    })
});

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct MemfdFlags: u32 {
        const MFD_CLOEXEC = 0x0001;
        const MFD_ALLOW_SEALING = 0x0002;
        const MFD_NOEXEC_SEAL = 0x0008;
        const MFD_EXEC = 0x0010;
    }
}

define_syscall!(MemfdCreate, |name: String, flags: u32| {
    let flags = MemfdFlags::from_bits(flags).ok_or(SyscallError::InvalidArguments)?;
    if flags.contains(MemfdFlags::MFD_NOEXEC_SEAL) && flags.contains(MemfdFlags::MFD_EXEC) {
        return Err(SyscallError::InvalidArguments);
    }

    let pid = get_current_process().lock().pid.0;
    let id = MEMFD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let sanitized_name = if name.is_empty() {
        String::from("anon")
    } else {
        name.replace('/', "_")
    };
    let path = Path::new(&format!("/memfd/{pid}-{id}-{sanitized_name}"));
    let object = create_memfd_object(
        path,
        sanitized_name,
        flags.intersects(MemfdFlags::MFD_ALLOW_SEALING | MemfdFlags::MFD_NOEXEC_SEAL),
    );
    let fd_flags = if flags.contains(MemfdFlags::MFD_CLOEXEC) {
        FdFlags::CLOEXEC
    } else {
        FdFlags::empty()
    };
    let fd = get_current_process()
        .lock()
        .push_object_with_flags(object, fd_flags);

    Ok(fd)
});

define_syscall!(
    MmapObject,
    |object: ObjectRef, pages: u64, offset: u64, protection: Protection| {
        let object = object.as_mappable()?;
        let address = object.map(offset, pages, protection)?;

        Ok(address.as_u64() as usize)
    }
);

define_syscall!(Lseek, |object: ObjectRef,
                        offset: i64,
                        seek_type: Whence| {
    trace_executor_mountinfo_object(&object, "lseek-enter");
    let result = object
        .clone()
        .as_seekable()?
        .seek(offset, seek_type)
        .map_err(SyscallError::from)?;
    trace_executor_mountinfo_object(&object, "lseek-exit");
    Ok(result)
});

define_syscall!(Pread64, |object: ObjectRef,
                          buf_ptr: *mut u8,
                          len: usize,
                          offset: i64| {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    trace_executor_mountinfo_object(&object, "pread-enter");
    let file = object.clone().as_file_like()?;
    let read = unsafe { file.read_at(slice::from_raw_parts_mut(buf_ptr, len), offset as u64)? };
    trace_executor_mountinfo_object(&object, "pread-exit");
    Ok(read)
});

define_syscall!(Pwrite64, |object: ObjectRef,
                           buf_ptr: *const u8,
                           len: usize,
                           offset: i64| {
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
