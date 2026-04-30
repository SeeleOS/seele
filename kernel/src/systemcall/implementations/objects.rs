use core::sync::atomic::{AtomicU64, Ordering};

use alloc::{collections::btree_map::BTreeMap, format, string::String, vec, vec::Vec};
use bitflags::bitflags;
use spin::Mutex;

use crate::{
    define_syscall,
    filesystem::vfs_traits::DirectoryContentType,
    filesystem::vfs_traits::Whence,
    filesystem::{info::DirectoryContentInfo, object::FileLikeObject, path::Path},
    memory::protection::Protection,
    memory::user_safe,
    misc::systemd_perf::{self, PerfBucket},
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
    socket::{InetSocketKind, UnixSocketKind},
    systemcall::utils::{SyscallError, SyscallImpl, SyscallResult},
};

static DIR_OFFSETS: Mutex<BTreeMap<(ProcessID, u64), usize>> = Mutex::new(BTreeMap::new());
static MEMFD_COUNTER: AtomicU64 = AtomicU64::new(0);
const COPY_CHUNK_SIZE: usize = 16 * 1024;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct FallocateFlags: i32 {
        const FALLOC_FL_KEEP_SIZE = 0x01;
        const FALLOC_FL_PUNCH_HOLE = 0x02;
        const FALLOC_FL_COLLAPSE_RANGE = 0x08;
        const FALLOC_FL_ZERO_RANGE = 0x10;
        const FALLOC_FL_INSERT_RANGE = 0x20;
        const FALLOC_FL_UNSHARE_RANGE = 0x40;
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct LinuxIovec {
    iov_base: *const u8,
    iov_len: usize,
}

fn copy_iovecs(iovs: &[LinuxIovec]) -> Result<Vec<u8>, SyscallError> {
    let total_len = iovs.iter().try_fold(0usize, |acc, iov| {
        acc.checked_add(iov.iov_len)
            .ok_or(SyscallError::InvalidArguments)
    })?;
    let mut buffer = Vec::with_capacity(total_len);
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }
        buffer.extend_from_slice(&user_safe::read_buffer(iov.iov_base, iov.iov_len)?);
    }
    Ok(buffer)
}

fn read_iovecs(iov_ptr: *const LinuxIovec, iovcnt: i32) -> Result<Vec<LinuxIovec>, SyscallError> {
    if iovcnt <= 0 {
        return Ok(Vec::new());
    }
    if iov_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut iovs = Vec::with_capacity(iovcnt as usize);
    for index in 0..iovcnt as usize {
        iovs.push(user_safe::read(unsafe { iov_ptr.add(index) })?);
    }
    Ok(iovs)
}

fn fallback_dirent_inode(info: &DirectoryContentInfo, offset: usize) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in info.name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^= match info.content_type {
        DirectoryContentType::Directory => 4,
        DirectoryContentType::File => 8,
        DirectoryContentType::Symlink => 10,
    };
    hash ^= offset as u64;
    hash.max(1)
}

fn log_sddm_dirents(_object_index: u64, _obj: &FileLikeObject, _contents: &[DirectoryContentInfo]) {
}

fn log_display_pipe_bytes(_op: &str, _object: &ObjectRef, _bytes: &[u8]) {}

fn log_display_write_dispatch(_op: &str, _object: &ObjectRef, _len: usize) {}

fn log_x_chain_write_bytes(_bytes: &[u8]) {}

fn log_user_manager_socket_bytes(_op: &str, _object: &ObjectRef, _bytes: &[u8]) {}

fn write_dirents64(object_index: u64, buf: *mut u8, len: usize) -> SyscallResult {
    let obj = get_object_current_process(object_index)?.as_file_like()?;
    let contents = obj.directory_contents().map_err(SyscallError::from)?;
    log_sddm_dirents(object_index, &obj, &contents);
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
            let inode = if info.inode != 0 {
                info.inode
            } else {
                fallback_dirent_inode(info, *offset_entry)
            };
            entry_ptr.cast::<u64>().write_unaligned(inode);
            entry_ptr
                .add(8)
                .cast::<i64>()
                .write_unaligned((*offset_entry as i64) + 1);
            entry_ptr.add(16).cast::<u16>().write_unaligned(reclen);
            let linux_type = match info.content_type {
                DirectoryContentType::Directory => 4,
                DirectoryContentType::File => 8,
                DirectoryContentType::Symlink => 10,
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
    systemd_perf::profile_current_process(PerfBucket::Getdents64, || {
        write_dirents64(object_index, buf, len)
    })
});

define_syscall!(Read, |object: ObjectRef, buf_ptr: *mut u8, len: usize| {
    let mut buffer = vec![0; len];
    let read = object.clone().as_readable()?.read(&mut buffer)?;
    if read > 0 {
        log_display_pipe_bytes("read", &object, &buffer[..read]);
        log_user_manager_socket_bytes("read", &object, &buffer[..read]);
        user_safe::write_buffer(buf_ptr, &buffer[..read])?;
    }
    Ok(read)
});

define_syscall!(Write, |object: ObjectRef, buf_ptr: *mut u8, len: usize| {
    let bytes = user_safe::read_buffer(buf_ptr.cast_const(), len)?;
    log_display_write_dispatch("write", &object, bytes.len());
    log_display_pipe_bytes("write", &object, &bytes);
    log_x_chain_write_bytes(&bytes);
    log_user_manager_socket_bytes("write", &object, &bytes);
    Ok(object.as_writable()?.write(&bytes)?)
});

define_syscall!(Writev, |object: ObjectRef,
                         iov_ptr: *const LinuxIovec,
                         iovcnt: i32| {
    if iovcnt < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let writable = object.clone().as_writable()?;
    let mut written = 0usize;
    if iovcnt > 0 && iov_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let iovs = read_iovecs(iov_ptr, iovcnt)?;
    let preserve_datagram_boundary = object
        .clone()
        .as_unix_socket()
        .map(|socket| {
            matches!(
                socket.kind,
                UnixSocketKind::Datagram | UnixSocketKind::SeqPacket
            )
        })
        .or_else(|_| {
            object
                .clone()
                .as_inet_socket()
                .map(|socket| socket.kind == InetSocketKind::Datagram)
        })
        .unwrap_or(false);
    if preserve_datagram_boundary {
        let buffer = copy_iovecs(&iovs)?;
        log_display_write_dispatch("writev", &object, buffer.len());
        log_display_pipe_bytes("writev", &object, &buffer);
        log_x_chain_write_bytes(&buffer);
        log_user_manager_socket_bytes("writev", &object, &buffer);
        return Ok(writable.write(&buffer)?);
    }

    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }
        let buf = user_safe::read_buffer(iov.iov_base, iov.iov_len)?;
        log_display_write_dispatch("writev", &object, buf.len());
        log_display_pipe_bytes("writev", &object, &buf);
        log_x_chain_write_bytes(&buf);
        log_user_manager_socket_bytes("writev", &object, &buf);
        let count = writable.write(&buf)?;
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
    if process.clear_fd_slot(object_num).is_ok() {
        DIR_OFFSETS.lock().remove(&(current_pid, object_num as u64));
        Ok(0)
    } else {
        Err(SyscallError::BadFileDescriptor)
    }
});

define_syscall!(Ioctl, |object: ObjectRef,
                        request: u64,
                        request_ptr: u64| {
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

define_syscall!(Ftruncate, |object: ObjectRef, length: i64| {
    if length < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    object
        .as_file_like()?
        .truncate(length as u64)
        .map_err(SyscallError::from)?;
    Ok(0)
});

define_syscall!(Fallocate, |object: ObjectRef,
                            mode: FallocateFlags,
                            offset: i64,
                            len: i64| {
    if offset < 0 || len < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if mode.bits()
        == (FallocateFlags::FALLOC_FL_KEEP_SIZE | FallocateFlags::FALLOC_FL_PUNCH_HOLE).bits()
    {
        return Ok(0);
    }
    if !mode.is_empty() {
        return Err(SyscallError::OperationNotSupported);
    }

    object
        .as_file_like()?
        .allocate(mode.bits() as u32, offset as u64, len as u64)
        .map_err(SyscallError::from)?;
    Ok(0)
});

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
    let raw_flags = flags;
    let flags = DupFlags::from_bits(flags).ok_or_else(|| {
        crate::s_println!("unsupported dup3 flags raw={:#x}", raw_flags);
        SyscallError::InvalidArguments
    })?;
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
        const CLOSE_RANGE_UNSHARE = 0x2;
        const CLOSE_RANGE_CLOEXEC = 0x4;
    }
}

define_syscall!(CloseRange, |first: usize, last: usize, flags: u32| {
    let raw_flags = flags;
    let flags = CloseRangeFlags::from_bits(flags).ok_or_else(|| {
        crate::s_println!("unsupported close_range flags raw={:#x}", raw_flags);
        SyscallError::InvalidArguments
    })?;
    if first > last {
        return Err(SyscallError::InvalidArguments);
    }

    let process_ref = get_current_process();
    let mut process = process_ref.lock();
    if first >= process.fd_table.len() {
        return Ok(0);
    }

    let end = last.min(process.fd_table.len().saturating_sub(1));
    for fd in first..=end {
        if process.fd_table[fd].is_none() {
            continue;
        }
        if flags.contains(CloseRangeFlags::CLOSE_RANGE_CLOEXEC) {
            process.set_fd_flags(fd, FdFlags::CLOEXEC)?;
        } else {
            process.clear_fd_slot(fd)?;
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
    let raw_flags = flags;
    let flags = MemfdFlags::from_bits(flags).ok_or_else(|| {
        crate::s_println!("unsupported memfd_create flags raw={:#x}", raw_flags);
        SyscallError::InvalidArguments
    })?;
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
    let result = object
        .clone()
        .as_seekable()?
        .seek(offset, seek_type)
        .map_err(SyscallError::from)?;
    Ok(result)
});

define_syscall!(Pread64, |object: ObjectRef,
                          buf_ptr: *mut u8,
                          len: usize,
                          offset: i64| {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let file = object.clone().as_file_like()?;
    let mut buffer = vec![0; len];
    let read = file.read_at(&mut buffer, offset as u64)?;
    if read > 0 {
        user_safe::write_buffer(buf_ptr, &buffer[..read])?;
    }
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
    let buffer = user_safe::read_buffer(buf_ptr, len)?;
    let written = writable.write(&buffer)?;
    let _ = seekable.seek(current, Whence::Start);
    Ok(written)
});
