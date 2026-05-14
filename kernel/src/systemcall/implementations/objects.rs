use core::{
    mem,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::memory::utils::Mut;
use alloc::{collections::btree_map::BTreeMap, format, string::String, vec, vec::Vec};
use bitflags::bitflags;

use crate::{
    define_syscall,
    filesystem::vfs_traits::DirectoryContentType,
    filesystem::vfs_traits::Whence,
    filesystem::{info::DirectoryContentInfo, object::FileLikeObject, path::Path},
    memory::protection::Protection,
    memory::user_safe,
    misc::profile::{self, HotSyscallPhase},
    object::{
        config::ConfigurateRequest,
        control::control_object,
        device::get_device,
        file_locks::flock_lock,
        linux_ioctl::{LinuxIoctlOp, LinuxIoctlTarget, socket_raw_ioctl_op},
        memfd::create_memfd_object,
        misc::{ObjectRef, get_object_current_process},
        traits::Readable,
    },
    process::{
        FdFlags,
        manager::get_current_process,
        misc::{ProcessID, with_current_process},
    },
    socket::{InetSocketKind, UnixSocketKind},
    systemcall::utils::{SyscallError, SyscallImpl, SyscallResult},
    thread::get_current_thread,
};

static DIR_OFFSETS: Mut<BTreeMap<(ProcessID, u64), usize>> = Mut::new(BTreeMap::new());
static MEMFD_COUNTER: AtomicU64 = AtomicU64::new(0);
const COPY_CHUNK_SIZE: usize = 16 * 1024;
const LINEAR_IO_CHUNK_SIZE: usize = 64 * 1024;

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

fn ioctl_target_for_object(object: &ObjectRef) -> Option<LinuxIoctlTarget> {
    if object.clone().as_drm_prime_buffer().is_ok() {
        Some(LinuxIoctlTarget::DrmPrime)
    } else if object.clone().as_netlink_socket().is_ok() {
        Some(LinuxIoctlTarget::NetlinkSocket)
    } else if object.clone().as_inet_socket().is_ok() {
        Some(LinuxIoctlTarget::InetSocket)
    } else if object.clone().as_unix_socket().is_ok() {
        Some(LinuxIoctlTarget::UnixSocket)
    } else if object.clone().as_pty_slave().is_ok() {
        Some(LinuxIoctlTarget::PtySlave)
    } else if object.clone().as_tty_device().is_ok() {
        Some(LinuxIoctlTarget::TtyDevice)
    } else {
        match object.debug_name() {
            "seele_os_linux::misc::fb_object::FramebufferObject" => {
                Some(LinuxIoctlTarget::Framebuffer)
            }
            "seele_os_linux::terminal::object::TerminalObject" => Some(LinuxIoctlTarget::Terminal),
            "seele_os_linux::terminal::pty::master::PtyMaster" => Some(LinuxIoctlTarget::PtyMaster),
            "seele_os_linux::drm::object::DrmCardObject" => Some(LinuxIoctlTarget::DrmCard),
            "seele_os_linux::evdev::object::EventDeviceClientObject" => {
                Some(LinuxIoctlTarget::EvdevClient)
            }
            _ => None,
        }
    }
}

fn write_dirents64(object_index: u64, buf: *mut u8, len: usize) -> SyscallResult {
    if buf.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let obj = get_object_current_process(object_index)?.as_file_like()?;
    let contents = obj.directory_contents().map_err(SyscallError::from)?;
    log_sddm_dirents(object_index, &obj, &contents);
    let current_pid = get_current_process().lock().pid;
    let mut offsets = DIR_OFFSETS.lock();
    let offset_entry = offsets.entry((current_pid, object_index)).or_insert(0usize);
    if *offset_entry >= contents.len() {
        return Ok(0);
    }
    if len < 24 {
        return Err(SyscallError::InvalidArguments);
    }
    let mut bytes_written = 0;

    while *offset_entry < contents.len() {
        let info = &contents[*offset_entry];
        let name_bytes = info.name.as_bytes();
        let reclen = ((20 + name_bytes.len() + 7) & !7) as u16;
        if bytes_written + reclen as usize > len {
            break;
        }

        let mut entry = vec![0u8; reclen as usize];
        let inode = if info.inode != 0 {
            info.inode
        } else {
            fallback_dirent_inode(info, *offset_entry)
        };
        entry[0..8].copy_from_slice(&inode.to_ne_bytes());
        entry[8..16].copy_from_slice(&((*offset_entry as i64) + 1).to_ne_bytes());
        entry[16..18].copy_from_slice(&reclen.to_ne_bytes());
        entry[18] = match info.content_type {
            DirectoryContentType::Directory => 4,
            DirectoryContentType::File => 8,
            DirectoryContentType::Symlink => 10,
        };
        entry[19..19 + name_bytes.len()].copy_from_slice(name_bytes);
        entry[19 + name_bytes.len()] = 0;
        user_safe::write_buffer(unsafe { buf.add(bytes_written) }, &entry)?;

        bytes_written += reclen as usize;
        *offset_entry += 1;
    }

    Ok(bytes_written)
}

fn read_object_at_offset(
    object: &ObjectRef,
    buffer: &mut [u8],
    offset: i64,
) -> SyscallResult<usize> {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    if let Ok(file) = object.clone().as_file_like() {
        return Ok(file.read_at(buffer, offset as u64)?);
    }

    let seekable = object.clone().as_seekable()?;
    let readable = object.clone().as_readable()?;
    let current = seekable.clone().seek(0, Whence::Current)? as i64;
    seekable.clone().seek(offset, Whence::Start)?;
    let read = readable.read(buffer)?;
    let _ = seekable.seek(current, Whence::Start);
    Ok(read)
}

fn write_object_at_offset(object: &ObjectRef, buffer: &[u8], offset: i64) -> SyscallResult<usize> {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    if let Ok(file) = object.clone().as_file_like() {
        return Ok(file.write_at(buffer, offset as u64)?);
    }

    let seekable = object.clone().as_seekable()?;
    let writable = object.clone().as_writable()?;
    let current = seekable.clone().seek(0, Whence::Current)? as i64;
    seekable.clone().seek(offset, Whence::Start)?;
    let written = writable.write(buffer)?;
    let _ = seekable.seek(current, Whence::Start);
    Ok(written)
}

fn readable_object_phase(object: &ObjectRef) -> HotSyscallPhase {
    if object.clone().as_tty_device().is_ok() {
        HotSyscallPhase::ReadReadableTty
    } else if object.clone().as_pty_slave().is_ok() {
        HotSyscallPhase::ReadReadablePtySlave
    } else if object.clone().as_unix_socket().is_ok() {
        HotSyscallPhase::ReadReadableUnixSocket
    } else if object.clone().as_inet_socket().is_ok() {
        HotSyscallPhase::ReadReadableInetSocket
    } else if object.clone().as_netlink_socket().is_ok() {
        HotSyscallPhase::ReadReadableNetlinkSocket
    } else if object.clone().as_fuse_device().is_ok() {
        HotSyscallPhase::ReadReadableFuseDevice
    } else {
        HotSyscallPhase::ReadReadableOther
    }
}

fn copy_between_objects_with_offsets(
    input: ObjectRef,
    input_offset: Option<*mut i64>,
    output: ObjectRef,
    output_offset: Option<*mut i64>,
    mut remaining: usize,
) -> SyscallResult {
    let readable = input.clone().as_readable()?;
    let writable = output.clone().as_writable()?;
    let mut buffer = [0u8; COPY_CHUNK_SIZE];
    let mut total = 0usize;

    while remaining > 0 {
        let chunk_len = remaining.min(buffer.len());

        let read = if let Some(offset_ptr) = input_offset {
            let offset = user_safe::read(offset_ptr)?;
            read_object_at_offset(&input, &mut buffer[..chunk_len], offset)?
        } else {
            readable.read(&mut buffer[..chunk_len])?
        };
        if read == 0 {
            break;
        }

        if let Some(offset_ptr) = input_offset {
            let offset = user_safe::read(offset_ptr)?;
            user_safe::write(offset_ptr, &(offset + read as i64))?;
        }

        let mut written = 0usize;
        while written < read {
            let count = if let Some(offset_ptr) = output_offset {
                let offset = user_safe::read(offset_ptr)?;
                write_object_at_offset(&output, &buffer[written..read], offset)?
            } else {
                writable.write(&buffer[written..read])?
            };
            if count == 0 {
                return Err(SyscallError::BrokenPipe);
            }

            if let Some(offset_ptr) = output_offset {
                let offset = user_safe::read(offset_ptr)?;
                user_safe::write(offset_ptr, &(offset + count as i64))?;
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

fn read_file_like_in_chunks(
    file: &FileLikeObject,
    buffer: &mut Vec<u8>,
    buf_ptr: *mut u8,
    len: usize,
) -> SyscallResult<usize> {
    if len == 0 {
        return Ok(0);
    }

    let scratch_len = len.min(LINEAR_IO_CHUNK_SIZE);
    if buffer.len() < scratch_len {
        buffer.resize(scratch_len, 0);
    }
    let mut total = 0usize;

    while total < len {
        let chunk_len = (len - total).min(buffer.len());
        let read = file.read(&mut buffer[..chunk_len])?;
        if read == 0 {
            break;
        }

        user_safe::write_buffer(unsafe { buf_ptr.add(total) }, &buffer[..read])?;
        total += read;
        if read < chunk_len {
            break;
        }
    }

    Ok(total)
}

fn write_object_in_chunks(
    object: &ObjectRef,
    buf_ptr: *const u8,
    len: usize,
) -> SyscallResult<usize> {
    if len == 0 {
        return Ok(0);
    }

    let writable = object.clone().as_writable()?;
    let mut total = 0usize;

    while total < len {
        let chunk_len = (len - total).min(LINEAR_IO_CHUNK_SIZE);
        let bytes = user_safe::read_buffer(unsafe { buf_ptr.add(total) }, chunk_len)?;
        log_display_write_dispatch("write", object, bytes.len());
        log_display_pipe_bytes("write", object, &bytes);
        log_x_chain_write_bytes(&bytes);
        log_user_manager_socket_bytes("write", object, &bytes);
        let written = writable.write(&bytes)?;
        total += written;
        if written < chunk_len {
            break;
        }
    }

    Ok(total)
}

fn pread_file_like_in_chunks(
    file: &FileLikeObject,
    buf_ptr: *mut u8,
    len: usize,
    offset: u64,
) -> SyscallResult<usize> {
    if len == 0 {
        return Ok(0);
    }

    let mut buffer = vec![0; len.min(LINEAR_IO_CHUNK_SIZE)];
    let mut total = 0usize;

    while total < len {
        let chunk_len = (len - total).min(buffer.len());
        let read = file.read_at(&mut buffer[..chunk_len], offset + total as u64)?;
        if read == 0 {
            break;
        }

        user_safe::write_buffer(unsafe { buf_ptr.add(total) }, &buffer[..read])?;
        total += read;
        if read < chunk_len {
            break;
        }
    }

    Ok(total)
}

fn pwrite_object_in_chunks(
    object: &ObjectRef,
    buf_ptr: *const u8,
    len: usize,
    offset: i64,
) -> SyscallResult<usize> {
    if len == 0 {
        return Ok(0);
    }

    if let Ok(file) = object.clone().as_file_like() {
        let mut total = 0usize;
        while total < len {
            let chunk_len = (len - total).min(LINEAR_IO_CHUNK_SIZE);
            let bytes = user_safe::read_buffer(unsafe { buf_ptr.add(total) }, chunk_len)?;
            let written = file.write_at(&bytes, offset as u64 + total as u64)?;
            total += written;
            if written < chunk_len {
                break;
            }
        }
        return Ok(total);
    }

    let seekable = object.clone().as_seekable()?;
    let writable = object.clone().as_writable()?;
    let current = seekable.clone().seek(0, Whence::Current)? as i64;
    seekable.clone().seek(offset, Whence::Start)?;

    let mut total = 0usize;
    while total < len {
        let chunk_len = (len - total).min(LINEAR_IO_CHUNK_SIZE);
        let bytes = user_safe::read_buffer(unsafe { buf_ptr.add(total) }, chunk_len)?;
        let written = writable.write(&bytes)?;
        total += written;
        if written < chunk_len {
            break;
        }
    }

    let _ = seekable.seek(current, Whence::Start);
    Ok(total)
}

define_syscall!(Getdents, |object_index: u64, buf: *mut u8, len: usize| {
    write_dirents64(object_index, buf, len)
});

define_syscall!(Getdents64, |object_index: u64, buf: *mut u8, len: usize| {
    write_dirents64(object_index, buf, len)
});

define_syscall!(Read, |object: ObjectRef, buf_ptr: *mut u8, len: usize| {
    let thread_ref = get_current_thread();
    let mut buffer = {
        let mut thread = thread_ref.lock();
        mem::take(&mut thread.io_buffer)
    };

    if let Ok(file) = object.clone().as_file_like() {
        let start = profile::scope_start();
        let result = read_file_like_in_chunks(&file, &mut buffer, buf_ptr, len);
        profile::record_hot_syscall_phase(
            HotSyscallPhase::ReadFileLike,
            profile::scope_start().saturating_sub(start),
        );
        {
            let mut thread = thread_ref.lock();
            thread.io_buffer = buffer;
        }
        return result;
    }

    if buffer.len() < len {
        buffer.resize(len, 0);
    }
    let readable_phase = readable_object_phase(&object);
    let readable_start = profile::scope_start();
    let result = object.clone().as_readable()?.read(&mut buffer[..len]);
    let readable_cycles = profile::scope_start().saturating_sub(readable_start);
    let syscall_blocked_cycles = {
        let thread = thread_ref.lock();
        thread.blocked_syscall_cycles
    };
    let active_readable_cycles = readable_cycles.saturating_sub(syscall_blocked_cycles);
    profile::record_hot_syscall_phase(HotSyscallPhase::ReadReadable, active_readable_cycles);
    profile::record_hot_syscall_phase(readable_phase, active_readable_cycles);

    let result = match result {
        Ok(read) => {
            if read > 0 {
                log_display_pipe_bytes("read", &object, &buffer[..read]);
                log_user_manager_socket_bytes("read", &object, &buffer[..read]);
                let copy_start = profile::scope_start();
                user_safe::write_buffer(buf_ptr, &buffer[..read])?;
                profile::record_hot_syscall_phase(
                    HotSyscallPhase::ReadCopyToUser,
                    profile::scope_start().saturating_sub(copy_start),
                );
            }
            Ok(read)
        }
        Err(err) => Err(err.into()),
    };

    {
        let mut thread = thread_ref.lock();
        thread.io_buffer = buffer;
    }

    result
});

define_syscall!(Write, |object: ObjectRef, buf_ptr: *mut u8, len: usize| {
    if object.clone().as_file_like().is_ok() {
        return write_object_in_chunks(&object, buf_ptr.cast_const(), len);
    }

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
    let writable = object.clone().as_writable()?;
    let buffer = copy_iovecs(&iovs)?;
    if preserve_datagram_boundary || !buffer.is_empty() {
        log_display_write_dispatch("writev", &object, buffer.len());
        log_display_pipe_bytes("writev", &object, &buffer);
        log_x_chain_write_bytes(&buffer);
        log_user_manager_socket_bytes("writev", &object, &buffer);
    }
    Ok(writable.write(&buffer)?)
});

define_syscall!(Sendfile, |out_fd: ObjectRef,
                           in_fd: ObjectRef,
                           offset: *mut i64,
                           count: usize| {
    copy_between_objects_with_offsets(
        in_fd,
        (!offset.is_null()).then_some(offset),
        out_fd,
        None,
        count,
    )
});

define_syscall!(CopyFileRange, |fd_in: ObjectRef,
                                off_in: *mut i64,
                                fd_out: ObjectRef,
                                off_out: *mut i64,
                                len: usize,
                                flags: u32| {
    if flags != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    copy_between_objects_with_offsets(
        fd_in,
        (!off_in.is_null()).then_some(off_in),
        fd_out,
        (!off_out.is_null()).then_some(off_out),
        len,
    )
});

define_syscall!(Splice, |fd_in: ObjectRef,
                         off_in: *mut i64,
                         fd_out: ObjectRef,
                         off_out: *mut i64,
                         len: usize,
                         flags: u32| {
    if flags != 0 {
        return Err(SyscallError::InvalidArguments);
    }

    copy_between_objects_with_offsets(
        fd_in,
        (!off_in.is_null()).then_some(off_in),
        fd_out,
        (!off_out.is_null()).then_some(off_out),
        len,
    )
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

define_syscall!(Ioctl, |fd: usize, request: u64, request_ptr: u64| {
    let object = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    let config_request = ConfigurateRequest::new(request, request_ptr)?;
    let ioctl_op = config_request.kind();
    let ioctl_target = ioctl_target_for_object(&object);
    let ioctl_start = profile::scope_start();
    let res = object.as_configuratable()?.configure(config_request);
    let ioctl_cycles = profile::scope_start().saturating_sub(ioctl_start);

    if let Some(op) = ioctl_op {
        profile::record_ioctl_op(op, ioctl_cycles);
    }
    if let Some(target) = ioctl_target {
        profile::record_ioctl_target(target, ioctl_cycles);
    }

    if matches!(socket_raw_ioctl_op(request), Some(LinuxIoctlOp::RawFioclex)) && res.is_ok() {
        let process_ref = get_current_process();
        let mut process = process_ref.lock();
        process
            .set_fd_flags(fd, FdFlags::CLOEXEC)
            .map_err(SyscallError::from)?;
    }

    res.map(|val| val as usize).map_err(Into::into)
});

define_syscall!(Fcntl, |fd: u64, command: u64, arg: u64| {
    control_object(fd, command, arg)
});

define_syscall!(Flock, |object: ObjectRef, operation: i32| {
    flock_lock(&object, operation)
});

fn flush_process_file_mappings() -> SyscallResult {
    let process_lock_start = profile::scope_start();
    let process = get_current_process();
    let mut process = process.lock();
    profile::record_hot_syscall_phase(
        HotSyscallPhase::FsyncProcessLock,
        profile::scope_start().saturating_sub(process_lock_start),
    );

    let flush_start = profile::scope_start();
    process
        .addrspace
        .flush_all_file_mappings()
        .map_err(SyscallError::from)?;
    profile::record_hot_syscall_phase(
        HotSyscallPhase::FsyncFlushMappings,
        profile::scope_start().saturating_sub(flush_start),
    );
    Ok(0)
}

define_syscall!(Fsync, |_object: ObjectRef| {
    flush_process_file_mappings()
});

define_syscall!(Fdatasync, |_object: ObjectRef| {
    flush_process_file_mappings()
});

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
    let source = get_object_current_process(source_fd as u64).map_err(SyscallError::from)?;
    if source_fd == dest {
        return Ok(dest);
    }

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
    let fd_table_len = process.fd_table.lock().len();
    if first >= fd_table_len {
        return Ok(0);
    }

    let end = last.min(fd_table_len.saturating_sub(1));
    for fd in first..=end {
        if process.fd_table.lock()[fd].is_none() {
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
    if matches!(seek_type, Whence::Start | Whence::End) && offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }
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
    pread_file_like_in_chunks(&file, buf_ptr, len, offset as u64)
});

define_syscall!(Pwrite64, |object: ObjectRef,
                           buf_ptr: *const u8,
                           len: usize,
                           offset: i64| {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    pwrite_object_in_chunks(&object, buf_ptr, len, offset)
});
