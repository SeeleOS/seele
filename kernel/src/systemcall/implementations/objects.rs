use core::{
    mem,
    sync::atomic::{AtomicU64, Ordering},
};

use alloc::{format, string::String, vec, vec::Vec};

use crate::{
    define_syscall,
    filesystem::vfs_traits::{DirectoryContentType, FileLikeType, LinuxFileAttributes, Whence},
    filesystem::{
        info::{DirectoryContentInfo, FileTimes},
        object::FileLikeObject,
        path::Path,
    },
    memory::protection::Protection,
    memory::user_safe,
    misc::c_types::CString,
    misc::profile::{self, HotSyscallPhase},
    object::{
        FileFlags,
        config::ConfigurateRequest,
        control::control_object,
        device::get_device,
        file_locks::{flock_lock, release_fd_entry_locks},
        linux_ioctl::{LinuxIoctlOp, LinuxIoctlTarget, socket_raw_ioctl_op},
        memfd::create_memfd_object,
        misc::{ObjectRef, get_object_current_process},
        traits::Readable,
    },
    process::{FdEntry, FdFlags, manager::get_current_process, misc::with_current_process},
    signal::{Signal, send_signal_to_process},
    systemcall::utils::{SyscallError, SyscallImpl, SyscallResult},
    thread::get_current_thread,
};

use super::{CloseRangeFlags, DupFlags, FallocateFlags, MemfdFlags, PositionedIoFlags};

static MEMFD_COUNTER: AtomicU64 = AtomicU64::new(0);
const COPY_CHUNK_SIZE: usize = 16 * 1024;
const LINEAR_IO_CHUNK_SIZE: usize = 64 * 1024;
const LINUX_IOV_MAX: i32 = 1024;
const MEMFD_NAME_MAX: usize = 249;
const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFBLK: u32 = 0o060000;
const S_IFREG: u32 = 0o100000;

#[derive(Clone, Copy)]
#[repr(C)]
struct LinuxIovec {
    iov_base: *const u8,
    iov_len: usize,
}

#[derive(Clone, Copy)]
enum PositionedIoOffset {
    Explicit(u64),
    Current,
}

fn iovec_total_len(iovs: &[LinuxIovec]) -> Result<usize, SyscallError> {
    iovs.iter().try_fold(0usize, |acc, iov| {
        let next = acc
            .checked_add(iov.iov_len)
            .ok_or(SyscallError::InvalidArguments)?;
        if next > isize::MAX as usize {
            return Err(SyscallError::InvalidArguments);
        }
        Ok(next)
    })
}

fn release_closed_fd_locks(pid: crate::process::misc::ProcessID, entries: Vec<FdEntry>) {
    for entry in entries {
        release_fd_entry_locks(pid, &entry);
    }
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

fn read_iovecs_for_syscall(
    iov_ptr: *const LinuxIovec,
    iovcnt: i32,
) -> Result<Vec<LinuxIovec>, SyscallError> {
    if !(0..=LINUX_IOV_MAX).contains(&iovcnt) {
        return Err(SyscallError::InvalidArguments);
    }

    if iovcnt > 0 && iov_ptr.is_null() {
        return Err(SyscallError::BadAddress);
    }

    read_iovecs(iov_ptr, iovcnt)
}

fn read_into_iovecs<F>(iovs: &[LinuxIovec], mut read_fn: F) -> SyscallResult<usize>
where
    F: FnMut(&mut [u8], usize) -> SyscallResult<usize>,
{
    let total_len = iovec_total_len(iovs)?;
    if total_len == 0 {
        return Ok(0);
    }

    let mut buffer = vec![0; total_len.min(LINEAR_IO_CHUNK_SIZE)];
    let mut total = 0usize;
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let mut copied_into_iov = 0usize;
        while copied_into_iov < iov.iov_len {
            let chunk_len = (iov.iov_len - copied_into_iov).min(buffer.len());
            let read = read_fn(&mut buffer[..chunk_len], total)?;
            if read == 0 {
                return Ok(total);
            }
            user_safe::write_buffer(
                unsafe { iov.iov_base.cast_mut().add(copied_into_iov) },
                &buffer[..read],
            )?;
            total += read;
            copied_into_iov += read;
            if read < chunk_len {
                return Ok(total);
            }
        }
    }

    Ok(total)
}

fn write_from_iovecs<F>(iovs: &[LinuxIovec], mut write_fn: F) -> SyscallResult<usize>
where
    F: FnMut(&[u8], usize) -> SyscallResult<usize>,
{
    let total_len = iovec_total_len(iovs)?;
    if total_len == 0 {
        return Ok(0);
    }

    let mut total = 0usize;
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let mut copied_from_iov = 0usize;
        while copied_from_iov < iov.iov_len {
            let chunk_len = (iov.iov_len - copied_from_iov).min(LINEAR_IO_CHUNK_SIZE);
            let bytes =
                user_safe::read_buffer(unsafe { iov.iov_base.add(copied_from_iov) }, chunk_len)?;
            let written = write_fn(&bytes, total)?;
            if written == 0 {
                return Err(SyscallError::NoSpaceLeft);
            }
            total += written;
            copied_from_iov += written;
            if written < chunk_len {
                return Ok(total);
            }
        }
    }

    Ok(total)
}

fn validate_iovecs_readable(iovs: &[LinuxIovec]) -> SyscallResult<()> {
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        if iov.iov_base.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let mut checked = 0usize;
        while checked < iov.iov_len {
            let chunk_len = (iov.iov_len - checked).min(LINEAR_IO_CHUNK_SIZE);
            let _ = user_safe::read_buffer(unsafe { iov.iov_base.add(checked) }, chunk_len)?;
            checked += chunk_len;
        }
    }
    Ok(())
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

fn directory_contents_with_dot_entries(
    obj: &FileLikeObject,
) -> SyscallResult<Vec<DirectoryContentInfo>> {
    let contents = obj.directory_contents().map_err(SyscallError::from)?;
    let mut entries = Vec::with_capacity(contents.len() + 2);
    if !contents.iter().any(|entry| entry.name == ".") {
        entries.push(DirectoryContentInfo::new(
            ".".into(),
            DirectoryContentType::Directory,
        ));
    }
    if !contents.iter().any(|entry| entry.name == "..") {
        entries.push(DirectoryContentInfo::new(
            "..".into(),
            DirectoryContentType::Directory,
        ));
    }
    entries.extend(contents);
    Ok(entries)
}

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
    let contents = directory_contents_with_dot_entries(&obj)?;
    let mut offset_entry = obj.directory_offset(contents.len());
    if offset_entry >= contents.len() {
        return Ok(0);
    }
    if len < 24 {
        return Err(SyscallError::InvalidArguments);
    }
    let mut bytes_written = 0;

    while offset_entry < contents.len() {
        let info = &contents[offset_entry];
        let name_bytes = info.name.as_bytes();
        let reclen = ((19 + name_bytes.len() + 1 + 7) & !7) as u16;
        if bytes_written + reclen as usize > len {
            if bytes_written == 0 {
                return Err(SyscallError::InvalidArguments);
            }
            break;
        }

        let mut entry = vec![0u8; reclen as usize];
        let inode = if info.inode != 0 {
            info.inode
        } else {
            fallback_dirent_inode(info, offset_entry)
        };
        entry[0..8].copy_from_slice(&inode.to_ne_bytes());
        entry[8..16].copy_from_slice(&((offset_entry as i64) + 1).to_ne_bytes());
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
        offset_entry += 1;
        obj.advance_directory_offset(1);
    }

    Ok(bytes_written)
}

fn write_dirents(object_index: u64, buf: *mut u8, len: usize) -> SyscallResult {
    if buf.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let obj = get_object_current_process(object_index)?.as_file_like()?;
    let contents = directory_contents_with_dot_entries(&obj)?;
    let mut offset_entry = obj.directory_offset(contents.len());
    if offset_entry >= contents.len() {
        return Ok(0);
    }
    if len < 24 {
        return Err(SyscallError::InvalidArguments);
    }
    let mut bytes_written = 0;

    while offset_entry < contents.len() {
        let info = &contents[offset_entry];
        let name_bytes = info.name.as_bytes();
        let reclen = ((20 + name_bytes.len() + 7) & !7) as u16;
        if bytes_written + reclen as usize > len {
            if bytes_written == 0 {
                return Err(SyscallError::InvalidArguments);
            }
            break;
        }

        let mut entry = vec![0u8; reclen as usize];
        let inode = if info.inode != 0 {
            info.inode
        } else {
            fallback_dirent_inode(info, offset_entry)
        };
        entry[0..8].copy_from_slice(&inode.to_ne_bytes());
        entry[8..16].copy_from_slice(&((offset_entry as u64) + 1).to_ne_bytes());
        entry[16..18].copy_from_slice(&reclen.to_ne_bytes());
        entry[18..18 + name_bytes.len()].copy_from_slice(name_bytes);
        entry[18 + name_bytes.len()] = 0;
        entry[reclen as usize - 1] = match info.content_type {
            DirectoryContentType::Directory => 4,
            DirectoryContentType::File => 8,
            DirectoryContentType::Symlink => 10,
        };
        user_safe::write_buffer(unsafe { buf.add(bytes_written) }, &entry)?;

        bytes_written += reclen as usize;
        offset_entry += 1;
        obj.advance_directory_offset(1);
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

fn object_current_offset(object: &ObjectRef) -> SyscallResult<i64> {
    Ok(object.clone().as_seekable()?.seek(0, Whence::Current)? as i64)
}

fn copy_file_offset(object: &ObjectRef, offset: Option<*mut i64>) -> SyscallResult<i64> {
    match offset {
        Some(offset) => user_safe::read(offset),
        None => object_current_offset(object),
    }
}

fn ranges_overlap(start_a: i64, start_b: i64, len: usize) -> bool {
    if len == 0 {
        return false;
    }
    let Ok(len) = i64::try_from(len) else {
        return true;
    };
    let Some(end_a) = start_a.checked_add(len) else {
        return true;
    };
    let Some(end_b) = start_b.checked_add(len) else {
        return true;
    };
    start_a < end_b && start_b < end_a
}

fn validate_copy_file_range(
    input: &ObjectRef,
    input_offset: Option<*mut i64>,
    output: &ObjectRef,
    output_offset: Option<*mut i64>,
    len: usize,
) -> SyscallResult<()> {
    let Ok(output_file) = output.clone().as_file_like() else {
        return Err(SyscallError::InvalidArguments);
    };
    let output_info = output_file.info()?;
    match output_info.file_like_type {
        FileLikeType::Directory => return Err(SyscallError::IsADirectory),
        FileLikeType::Symlink => return Err(SyscallError::InvalidArguments),
        FileLikeType::File => {}
    }
    if output_file.is_device_backed() {
        return Err(SyscallError::InvalidArguments);
    }
    ensure_object_writable(output)?;
    if output.clone().get_flags()?.contains(FileFlags::APPEND) {
        return Err(SyscallError::BadFileDescriptor);
    }
    if output_file
        .linux_file_attributes()?
        .contains(LinuxFileAttributes::FS_IMMUTABLE_FL)
    {
        return Err(SyscallError::PermissionDenied);
    }

    let Ok(input_file) = input.clone().as_file_like() else {
        return Err(SyscallError::InvalidArguments);
    };
    let input_info = input_file.info()?;
    if !matches!(input_info.file_like_type, FileLikeType::File) || input_file.is_device_backed() {
        return Err(SyscallError::InvalidArguments);
    }
    ensure_object_readable(input)?;

    let output_start = copy_file_offset(output, output_offset)?;
    if output_start < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if len > i64::MAX as usize {
        return Err(SyscallError::ValueTooLarge);
    }
    if output_start
        .checked_add(len as i64)
        .is_none_or(|end| end < 0)
    {
        return Err(SyscallError::FileTooLarge);
    }

    if input_file.mount_id() == output_file.mount_id() && input_info.inode == output_info.inode {
        let input_start = copy_file_offset(input, input_offset)?;
        if input_start < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if ranges_overlap(input_start, output_start, len) {
            return Err(SyscallError::InvalidArguments);
        }
    }

    Ok(())
}

fn update_copy_file_range_output_times(output: &ObjectRef) -> SyscallResult<()> {
    let output_file = output.clone().as_file_like()?;
    let mut times = output_file.info()?.times;
    let now = FileTimes::now();
    times.mtime_sec = now.mtime_sec;
    times.mtime_nsec = now.mtime_nsec;
    times.ctime_sec = now.ctime_sec;
    times.ctime_nsec = now.ctime_nsec;
    output_file.set_times(times, true)?;
    Ok(())
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
            let new_offset = offset
                .checked_add(read as i64)
                .ok_or(SyscallError::ValueTooLarge)?;
            user_safe::write(offset_ptr, &new_offset)?;
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
                let new_offset = offset
                    .checked_add(count as i64)
                    .ok_or(SyscallError::ValueTooLarge)?;
                user_safe::write(offset_ptr, &new_offset)?;
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
        let written = writable.write(&bytes)?;
        if written == 0 {
            return Err(SyscallError::NoSpaceLeft);
        }
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
            if written == 0 {
                return Err(SyscallError::NoSpaceLeft);
            }
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
        if written == 0 {
            return Err(SyscallError::NoSpaceLeft);
        }
        total += written;
        if written < chunk_len {
            break;
        }
    }

    let _ = seekable.seek(current, Whence::Start);
    Ok(total)
}

fn preadv_file_like(
    file: &FileLikeObject,
    iovs: &[LinuxIovec],
    offset: u64,
) -> SyscallResult<usize> {
    read_into_iovecs(iovs, |buffer, total| {
        file.read_at(buffer, offset + total as u64)
            .map_err(SyscallError::from)
    })
}

fn pwritev_object(object: &ObjectRef, iovs: &[LinuxIovec], offset: i64) -> SyscallResult<usize> {
    if let Ok(file) = object.clone().as_file_like() {
        return write_from_iovecs(iovs, |bytes, total| {
            file.write_at(bytes, offset as u64 + total as u64)
                .map_err(SyscallError::from)
        });
    }

    let seekable = object.clone().as_seekable()?;
    let writable = object.clone().as_writable()?;
    let current = seekable.clone().seek(0, Whence::Current)? as i64;
    seekable.clone().seek(offset, Whence::Start)?;
    let result = write_from_iovecs(iovs, |bytes, _total| {
        writable.write(bytes).map_err(SyscallError::from)
    });
    let _ = seekable.seek(current, Whence::Start);
    result
}

fn positioned_io_offset(offset: i64) -> SyscallResult<PositionedIoOffset> {
    if offset == -1 {
        return Ok(PositionedIoOffset::Current);
    }
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    Ok(PositionedIoOffset::Explicit(offset as u64))
}

fn check_positioned_io_flags(flags: PositionedIoFlags) -> SyscallResult<()> {
    if flags.bits() & !PositionedIoFlags::all().bits() != 0 {
        return Err(SyscallError::OperationNotSupported);
    }
    if flags.contains(PositionedIoFlags::RWF_HIPRI) {
        return Err(SyscallError::OperationNotSupported);
    }
    Ok(())
}

fn positioned_io_flags_from_raw(raw: u64) -> SyscallResult<PositionedIoFlags> {
    let flags = PositionedIoFlags::from_bits_retain(raw as i32);
    check_positioned_io_flags(flags)?;
    Ok(flags)
}

fn decode_preadv2_args(raw_arg5: u64, raw_arg6: u64) -> SyscallResult<PositionedIoFlags> {
    let raw_arg5_flags = raw_arg5 as i32;
    if raw_arg5_flags != 0 && raw_arg5_flags & !PositionedIoFlags::all().bits() == 0 {
        return positioned_io_flags_from_raw(raw_arg5);
    }
    positioned_io_flags_from_raw(raw_arg6)
}

fn ensure_object_readable(object: &ObjectRef) -> SyscallResult<()> {
    if object.clone().as_file_like().is_ok()
        && object.clone().get_flags()?.contains(FileFlags::WRONLY)
    {
        Err(SyscallError::BadFileDescriptor)
    } else {
        Ok(())
    }
}

fn ensure_object_writable(object: &ObjectRef) -> SyscallResult<()> {
    if object.clone().as_file_like().is_err() {
        return Ok(());
    }
    let flags = object.clone().get_flags()?;
    if flags.contains(FileFlags::WRONLY) || flags.contains(FileFlags::RDWR) {
        Ok(())
    } else {
        Err(SyscallError::BadFileDescriptor)
    }
}

define_syscall!(Getdents, |object_index: u64, buf: *mut u8, len: usize| {
    write_dirents(object_index, buf, len)
});

define_syscall!(Getdents64, |object_index: u64, buf: *mut u8, len: usize| {
    write_dirents64(object_index, buf, len)
});

define_syscall!(Read, |object: ObjectRef, buf_ptr: *mut u8, len: usize| {
    ensure_object_readable(&object)?;
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
    ensure_object_writable(&object)?;
    if object.clone().as_file_like().is_ok() {
        return write_object_in_chunks(&object, buf_ptr.cast_const(), len);
    }

    let bytes = user_safe::read_buffer(buf_ptr.cast_const(), len)?;
    let writable = object.as_writable()?;
    match writable.write(&bytes) {
        Ok(written) => Ok(written),
        Err(err) => {
            let syscall_err = SyscallError::from(err);
            if syscall_err == SyscallError::BrokenPipe {
                send_signal_to_process(&get_current_process(), Signal::SIGPIPE);
            }
            Err(syscall_err)
        }
    }
});

define_syscall!(Readv, |object: ObjectRef,
                        iov_ptr: *const LinuxIovec,
                        iovcnt: i32| {
    ensure_object_readable(&object)?;
    let iovs = read_iovecs_for_syscall(iov_ptr, iovcnt)?;
    let total_len = iovec_total_len(&iovs)?;
    if total_len == 0 {
        return Ok(0);
    }

    if let Ok(file) = object.clone().as_file_like() {
        return read_into_iovecs(&iovs, |buffer, _total| {
            file.read(buffer).map_err(SyscallError::from)
        });
    }

    let readable = object.as_readable()?;
    read_into_iovecs(&iovs, |buffer, _total| {
        readable.read(buffer).map_err(SyscallError::from)
    })
});

define_syscall!(Writev, |object: ObjectRef,
                         iov_ptr: *const LinuxIovec,
                         iovcnt: i32| {
    ensure_object_writable(&object)?;
    let iovs = read_iovecs_for_syscall(iov_ptr, iovcnt)?;
    validate_iovecs_readable(&iovs)?;
    let writable = object.clone().as_writable()?;
    match write_from_iovecs(&iovs, |bytes, _total| {
        writable.write(bytes).map_err(SyscallError::from)
    }) {
        Ok(written) => Ok(written),
        Err(syscall_err) => {
            if syscall_err == SyscallError::BrokenPipe {
                send_signal_to_process(&get_current_process(), Signal::SIGPIPE);
            }
            Err(syscall_err)
        }
    }
});

define_syscall!(Preadv, |object: ObjectRef,
                         iov_ptr: *const LinuxIovec,
                         iovcnt: i32,
                         offset: i64| {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    ensure_object_readable(&object)?;
    let iovs = read_iovecs_for_syscall(iov_ptr, iovcnt)?;
    let file = object
        .clone()
        .as_file_like()
        .map_err(|_| SyscallError::IllegalSeek)?;
    preadv_file_like(&file, &iovs, offset as u64)
});

define_syscall!(Pwritev, |object: ObjectRef,
                          iov_ptr: *const LinuxIovec,
                          iovcnt: i32,
                          offset: i64| {
    if offset < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    ensure_object_writable(&object)?;
    let iovs = read_iovecs_for_syscall(iov_ptr, iovcnt)?;
    pwritev_object(&object, &iovs, offset)
});

define_syscall!(Preadv2, |object: ObjectRef,
                          iov_ptr: *const LinuxIovec,
                          iovcnt: i32,
                          raw_offset: i64,
                          raw_arg5: u64,
                          raw_arg6: u64| {
    let flags = decode_preadv2_args(raw_arg5, raw_arg6)?;
    let offset = positioned_io_offset(raw_offset)?;
    ensure_object_readable(&object)?;
    let iovs = read_iovecs_for_syscall(iov_ptr, iovcnt)?;
    match offset {
        PositionedIoOffset::Explicit(offset) => {
            let file = object
                .clone()
                .as_file_like()
                .map_err(|_| SyscallError::IllegalSeek)?;
            if flags.contains(PositionedIoFlags::RWF_NOWAIT) {
                return Err(SyscallError::OperationNotSupported);
            }
            preadv_file_like(&file, &iovs, offset)
        }
        PositionedIoOffset::Current => {
            if flags.contains(PositionedIoFlags::RWF_NOWAIT) {
                return Err(SyscallError::OperationNotSupported);
            }
            if let Ok(file) = object.clone().as_file_like() {
                read_into_iovecs(&iovs, |buffer, _total| {
                    file.read(buffer).map_err(SyscallError::from)
                })
            } else {
                let readable = object.as_readable()?;
                read_into_iovecs(&iovs, |buffer, _total| {
                    readable.read(buffer).map_err(SyscallError::from)
                })
            }
        }
    }
});

define_syscall!(Pwritev2, |object: ObjectRef,
                           iov_ptr: *const LinuxIovec,
                           iovcnt: i32,
                           raw_offset: i64,
                           raw_arg5: u64,
                           raw_arg6: u64| {
    let flags = decode_preadv2_args(raw_arg5, raw_arg6)?;
    let offset = positioned_io_offset(raw_offset)?;
    ensure_object_writable(&object)?;
    let iovs = read_iovecs_for_syscall(iov_ptr, iovcnt)?;
    match offset {
        PositionedIoOffset::Explicit(offset) => {
            if flags.contains(PositionedIoFlags::RWF_APPEND) {
                let seekable = object.clone().as_seekable()?;
                let end = seekable.seek(0, Whence::End)? as i64;
                pwritev_object(&object, &iovs, end)
            } else {
                pwritev_object(&object, &iovs, offset as i64)
            }
        }
        PositionedIoOffset::Current if flags.contains(PositionedIoFlags::RWF_APPEND) => {
            let seekable = object.clone().as_seekable()?;
            let end = seekable.seek(0, Whence::End)? as i64;
            pwritev_object(&object, &iovs, end)
        }
        PositionedIoOffset::Current => {
            let writable = object.clone().as_writable()?;
            match write_from_iovecs(&iovs, |bytes, _total| {
                writable.write(bytes).map_err(SyscallError::from)
            }) {
                Ok(written) => Ok(written),
                Err(syscall_err) => {
                    if syscall_err == SyscallError::BrokenPipe {
                        send_signal_to_process(&get_current_process(), Signal::SIGPIPE);
                    }
                    Err(syscall_err)
                }
            }
        }
    }
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

    let input_offset = (!off_in.is_null()).then_some(off_in);
    let output_offset = (!off_out.is_null()).then_some(off_out);
    validate_copy_file_range(&fd_in, input_offset, &fd_out, output_offset, len)?;
    let copied =
        copy_between_objects_with_offsets(fd_in, input_offset, fd_out.clone(), output_offset, len)?;
    if copied > 0 {
        update_copy_file_range_output_times(&fd_out)?;
    }
    Ok(copied)
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
    if process.clear_fd_slot(object_num).is_ok() {
        Ok(0)
    } else {
        Err(SyscallError::BadFileDescriptor)
    }
});

define_syscall!(Ioctl, |fd: usize, request: u64, request_ptr: u64| {
    let object = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    if matches!(socket_raw_ioctl_op(request), Some(LinuxIoctlOp::RawFionbio)) {
        let nonblocking: i32 = user_safe::read(request_ptr as *const i32)?;
        let mut flags = object.clone().get_flags().map_err(SyscallError::from)?;
        if nonblocking != 0 {
            flags.insert(FileFlags::NONBLOCK);
        } else {
            flags.remove(FileFlags::NONBLOCK);
        }
        object.set_flags(flags).map_err(SyscallError::from)?;
        return Ok(0);
    }
    if matches!(socket_raw_ioctl_op(request), Some(LinuxIoctlOp::RawFioclex)) {
        get_current_process()
            .lock()
            .set_fd_flags(fd, FdFlags::CLOEXEC)
            .map_err(SyscallError::from)?;
        return Ok(0);
    }

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

    res.map(|val| val as usize).map_err(Into::into)
});

define_syscall!(Fcntl, |fd: u64, command: u64, arg: u64| {
    control_object(fd, command, arg)
});

define_syscall!(Flock, |object: ObjectRef, operation: i32| {
    flock_lock(&object, operation)
});

fn check_syncable_file(object: &ObjectRef) -> Result<(), SyscallError> {
    let stat = object.clone().as_statable()?.stat();
    match stat.st_mode & S_IFMT {
        S_IFREG | S_IFDIR | S_IFBLK => Ok(()),
        _ => Err(SyscallError::InvalidArguments),
    }
}

define_syscall!(Fsync, |object: ObjectRef| {
    check_syncable_file(&object)?;
    let file_like = object.as_file_like()?;
    crate::filesystem::vfs::VirtualFS
        .lock()
        .sync_path(file_like.path())?;
    Ok(0)
});

define_syscall!(Fdatasync, |object: ObjectRef| {
    check_syncable_file(&object)?;
    let file_like = object.as_file_like()?;
    crate::filesystem::vfs::VirtualFS
        .lock()
        .sync_path(file_like.path())?;
    Ok(0)
});

define_syscall!(Fadvise64, |object: ObjectRef,
                            _offset: i64,
                            _len: i64,
                            advice: i32| {
    if !(0..=5).contains(&advice) {
        return Err(SyscallError::InvalidArguments);
    }
    if object.clone().as_seekable().is_err() && object.clone().as_file_like().is_err() {
        return Err(SyscallError::IllegalSeek);
    }
    Ok(0)
});

define_syscall!(Ftruncate, |object: ObjectRef, length: i64| {
    if length < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    let stat = object.clone().as_statable()?.stat();
    if stat.st_mode & S_IFMT != S_IFREG {
        return Err(SyscallError::InvalidArguments);
    }
    let flags = object.clone().get_flags()?;
    if !flags.intersects(FileFlags::WRONLY | FileFlags::RDWR) {
        return Err(SyscallError::InvalidArguments);
    }

    let file_like = object
        .as_file_like()
        .map_err(|_| SyscallError::InvalidArguments)?;
    file_like
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
    let punch_hole = FallocateFlags::FALLOC_FL_KEEP_SIZE | FallocateFlags::FALLOC_FL_PUNCH_HOLE;
    if !mode.is_empty() && mode.bits() != punch_hole.bits() {
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
    if !get_current_process().lock().fd_within_limit(dest) {
        return Err(SyscallError::BadFileDescriptor);
    }
    if source_fd == dest {
        return Ok(dest);
    }

    get_current_process()
        .lock()
        .clone_object_to(source, dest)
        .map_err(Into::into)
});

define_syscall!(Dup3, |source_fd: usize, dest: usize, flags: DupFlags| {
    if source_fd == dest {
        return Err(SyscallError::InvalidArguments);
    }
    if !get_current_process().lock().fd_within_limit(dest) {
        return Err(SyscallError::BadFileDescriptor);
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

define_syscall!(
    CloseRange,
    |first: usize, last: usize, flags: CloseRangeFlags| {
        if first > last {
            return Err(SyscallError::InvalidArguments);
        }
        let allowed = CloseRangeFlags::CLOSE_RANGE_UNSHARE | CloseRangeFlags::CLOSE_RANGE_CLOEXEC;
        if flags.bits() & !allowed.bits() != 0 {
            return Err(SyscallError::InvalidArguments);
        }

        let process_ref = get_current_process();
        let mut process = process_ref.lock();
        if flags.contains(CloseRangeFlags::CLOSE_RANGE_UNSHARE) {
            process.unshare_fd_table();
        }

        let mut closed_entries = Vec::new();
        let pid = process.pid;
        {
            let mut fd_table = process.fd_table.lock();
            if first >= fd_table.len() {
                return Ok(0);
            }

            let end = last.min(fd_table.len().saturating_sub(1));
            for fd in first..=end {
                let Some(entry) = fd_table.get_mut(fd).and_then(Option::as_mut) else {
                    continue;
                };
                if flags.contains(CloseRangeFlags::CLOSE_RANGE_CLOEXEC) {
                    entry.fd_flags = FdFlags::CLOEXEC;
                } else if let Some(entry) = fd_table[fd].take() {
                    closed_entries.push(entry);
                }
            }
        }

        release_closed_fd_locks(pid, closed_entries);

        Ok(0)
    }
);

define_syscall!(OpenDevice, |name: String| {
    with_current_process(|process| {
        let device = get_device(name)?;
        let slot = process.push_object(device);

        Ok(slot)
    })
});

fn memfd_name_from_raw(name: CString) -> Result<String, SyscallError> {
    if name.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let mut out = String::new();
    for offset in 0..=MEMFD_NAME_MAX {
        let byte =
            user_safe::read(unsafe { name.add(offset) }).map_err(|_| SyscallError::BadAddress)?;
        if byte == 0 {
            return Ok(out);
        }
        if offset == MEMFD_NAME_MAX {
            return Err(SyscallError::InvalidArguments);
        }
        out.push(byte as char);
    }

    Err(SyscallError::InvalidArguments)
}

define_syscall!(MemfdCreate, |name: CString, flags: MemfdFlags| {
    if flags.contains(MemfdFlags::MFD_NOEXEC_SEAL) && flags.contains(MemfdFlags::MFD_EXEC) {
        return Err(SyscallError::InvalidArguments);
    }
    if flags.intersects(MemfdFlags::MFD_HUGE_MASK) && !flags.contains(MemfdFlags::MFD_HUGETLB) {
        return Err(SyscallError::InvalidArguments);
    }
    if flags.contains(MemfdFlags::MFD_HUGETLB) {
        return Err(SyscallError::NoDevice);
    }

    let name = memfd_name_from_raw(name)?;
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
    if matches!(seek_type, Whence::Start) && offset < 0 {
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
    ensure_object_readable(&object)?;
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

    ensure_object_writable(&object)?;
    pwrite_object_in_chunks(&object, buf_ptr, len, offset)
});

#[cfg(test)]
mod tests {
    use crate::{
        object::{
            FileFlags, config::LinuxTermios, file_locks::LinuxFlock,
            misc::get_object_current_process,
        },
        process::FdFlags,
        systemcall::{
            implementations::{
                CloseRange, CreatePty, Eventfd, Fcntl, Ioctl, SchedSetscheduler, Socket,
            },
            test::{
                TestLinuxSchedParam, assert_fd_flags, assert_object_flags, close_test_fd,
                expect_fd, occupied_fd_count,
            },
            test_helpers::{
                SyscallArgs, allocate_user_test_page, assert_linux_layout, expect_errno, expect_ok,
                read_user_value, write_user_value,
            },
            utils::SyscallError,
        },
    };

    crate::test!(
        close_range_syscalls,
        "close_range follows linux fd rules",
        close_range_syscalls_follow_linux_rules
    );
    crate::test!(
        object_control_syscalls,
        "ioctl and sched_setscheduler follow linux rules",
        object_control_syscalls_follow_linux_rules
    );

    fn close_range_syscalls_follow_linux_rules() {
        const CLOSE_RANGE_CLOEXEC: u64 = 0x4;

        let base_count = occupied_fd_count();
        let fd0 = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let fd1 = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let fd2 = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        let fd3 = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());

        expect_ok(
            SyscallArgs::new([fd1 as u64, fd2 as u64, CLOSE_RANGE_CLOEXEC, 0, 0, 0])
                .call::<CloseRange>(),
            0,
        );
        assert_fd_flags(fd0, FdFlags::empty());
        assert_fd_flags(fd1, FdFlags::CLOEXEC);
        assert_fd_flags(fd2, FdFlags::CLOEXEC);
        assert_fd_flags(fd3, FdFlags::empty());
        assert_eq!(occupied_fd_count(), base_count + 4);

        expect_ok(
            SyscallArgs::new([fd1 as u64, fd2 as u64, 0, 0, 0, 0]).call::<CloseRange>(),
            0,
        );
        assert!(get_object_current_process(fd1 as u64).is_err());
        assert!(get_object_current_process(fd2 as u64).is_err());
        assert_eq!(occupied_fd_count(), base_count + 2);

        expect_errno(
            SyscallArgs::new([fd0 as u64, fd3 as u64, 1, 0, 0, 0]).call::<CloseRange>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([fd3 as u64, fd0 as u64, 0, 0, 0, 0]).call::<CloseRange>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([4096, 8192, 0, 0, 0, 0]).call::<CloseRange>(),
            0,
        );

        close_test_fd(fd0);
        close_test_fd(fd3);
    }

    fn object_control_syscalls_follow_linux_rules() {
        const F_GETLK: u64 = 5;
        const F_RDLCK: i16 = 0;
        const F_UNLCK: i16 = 2;
        const TCGETS: u64 = 0x5401;
        const TIOCSPTLCK: u64 = 0x4004_5431;
        const TIOCGPTN: u64 = 0x8004_5430;
        const TIOCOUTQ: u64 = 0x5411;
        const FIONBIO: u64 = 0x5421;
        const FIOCLEX: u64 = 0x5451;
        const SOCK_STREAM: u64 = 1;
        const SOCK_RAW: u64 = 3;
        const AF_UNIX: u64 = 1;
        const AF_NETLINK: u64 = 16;
        const NETLINK_ROUTE: u64 = 0;
        const SCHED_OTHER: u64 = 0;
        const SCHED_FIFO: u64 = 1;

        assert_linux_layout::<LinuxTermios>(36, 4);
        assert_linux_layout::<TestLinuxSchedParam>(4, 4);

        let page = allocate_user_test_page();
        let eventfd = expect_fd(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Eventfd>());
        write_user_value(
            page + 1024,
            &LinuxFlock {
                lock_type: F_RDLCK,
                whence: 0,
                start: 0,
                len: 0,
                pid: 1234,
                __reserved: 0,
            },
        );
        expect_ok(
            SyscallArgs::new([eventfd as u64, F_GETLK, page + 1024, 0, 0, 0]).call::<Fcntl>(),
            0,
        );
        let no_conflict_lock = read_user_value::<LinuxFlock>(page + 1024);
        assert_eq!(no_conflict_lock.lock_type, F_UNLCK);
        assert_eq!(no_conflict_lock.pid, 1234);

        let [master_fd, slave_fd] = {
            write_user_value(page + 896, &0i32);
            write_user_value(page + 900, &0i32);
            expect_ok(
                SyscallArgs::new([page + 896, page + 900, 0, 0, 0, 0]).call::<CreatePty>(),
                0,
            );
            [
                read_user_value::<i32>(page + 896) as usize,
                read_user_value::<i32>(page + 900) as usize,
            ]
        };

        expect_ok(
            SyscallArgs::new([slave_fd as u64, TCGETS, page, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        let termios = read_user_value::<LinuxTermios>(page);
        assert_eq!(termios.c_cc.len(), 19);

        write_user_value(page + 128, &1i32);
        expect_ok(
            SyscallArgs::new([master_fd as u64, TIOCSPTLCK, page + 128, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([master_fd as u64, TIOCSPTLCK, 1, 0, 0, 0]).call::<Ioctl>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([usize::MAX as u64, TCGETS, page, 0, 0, 0]).call::<Ioctl>(),
            SyscallError::BadFileDescriptor,
        );

        let unix_socket =
            expect_fd(SyscallArgs::new([AF_UNIX, SOCK_STREAM, 0, 0, 0, 0]).call::<Socket>());
        assert_fd_flags(unix_socket, FdFlags::empty());
        write_user_value(page + 384, &1i32);
        expect_ok(
            SyscallArgs::new([unix_socket as u64, FIONBIO, page + 384, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        assert_object_flags(unix_socket, FileFlags::NONBLOCK);
        expect_ok(
            SyscallArgs::new([unix_socket as u64, FIOCLEX, 0, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        assert_fd_flags(unix_socket, FdFlags::CLOEXEC);
        expect_errno(
            SyscallArgs::new([unix_socket as u64, FIONBIO, 1, 0, 0, 0]).call::<Ioctl>(),
            SyscallError::BadAddress,
        );
        expect_ok(
            SyscallArgs::new([unix_socket as u64, TIOCOUTQ, page + 392, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        assert_eq!(read_user_value::<i32>(page + 392), 0);
        expect_errno(
            SyscallArgs::new([unix_socket as u64, TIOCGPTN, page + 392, 0, 0, 0]).call::<Ioctl>(),
            SyscallError::InappropriateIoctl,
        );

        let netlink_socket = expect_fd(
            SyscallArgs::new([AF_NETLINK, SOCK_RAW, NETLINK_ROUTE, 0, 0, 0]).call::<Socket>(),
        );
        expect_ok(
            SyscallArgs::new([netlink_socket as u64, FIOCLEX, 0, 0, 0, 0]).call::<Ioctl>(),
            0,
        );
        assert_fd_flags(netlink_socket, FdFlags::CLOEXEC);
        close_test_fd(netlink_socket);
        close_test_fd(unix_socket);

        write_user_value(page + 256, &TestLinuxSchedParam { sched_priority: 0 });
        expect_ok(
            SyscallArgs::new([0, SCHED_OTHER, page + 256, 0, 0, 0]).call::<SchedSetscheduler>(),
            0,
        );
        write_user_value(page + 260, &TestLinuxSchedParam { sched_priority: 1 });
        expect_ok(
            SyscallArgs::new([0, SCHED_FIFO, page + 260, 0, 0, 0]).call::<SchedSetscheduler>(),
            0,
        );
        write_user_value(page + 264, &TestLinuxSchedParam { sched_priority: 0 });
        expect_errno(
            SyscallArgs::new([0, SCHED_FIFO, page + 264, 0, 0, 0]).call::<SchedSetscheduler>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, SCHED_OTHER, page + 256, 0, 0, 0])
                .call::<SchedSetscheduler>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, SCHED_OTHER, 0, 0, 0, 0]).call::<SchedSetscheduler>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([0, 99, page + 256, 0, 0, 0]).call::<SchedSetscheduler>(),
            SyscallError::InvalidArguments,
        );

        close_test_fd(master_fd);
        close_test_fd(slave_fd);
        close_test_fd(eventfd);
    }
}
