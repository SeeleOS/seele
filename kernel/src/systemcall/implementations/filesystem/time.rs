use super::*;
use crate::filesystem::info::FileTimes;

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxTimeval {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxUtimbuf {
    actime: i64,
    modtime: i64,
}

#[derive(Clone, Copy)]
enum RequestedTime {
    Now,
    Omit,
    Set { sec: i64, nsec: i64 },
}

impl RequestedTime {
    fn from_timespec(timespec: LinuxTimespec) -> Result<Self, SyscallError> {
        match timespec.tv_nsec {
            UTIME_NOW => Ok(Self::Now),
            UTIME_OMIT => Ok(Self::Omit),
            0..=999_999_999 if timespec.tv_sec >= 0 => Ok(Self::Set {
                sec: timespec.tv_sec,
                nsec: timespec.tv_nsec,
            }),
            _ => Err(SyscallError::InvalidArguments),
        }
    }

    fn from_seconds(sec: i64) -> Result<Self, SyscallError> {
        if sec < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        Ok(Self::Set { sec, nsec: 0 })
    }

    fn from_timeval(timeval: LinuxTimeval) -> Result<Self, SyscallError> {
        if timeval.tv_sec < 0 || !(0..1_000_000).contains(&timeval.tv_usec) {
            return Err(SyscallError::InvalidArguments);
        }
        Ok(Self::Set {
            sec: timeval.tv_sec,
            nsec: timeval.tv_usec * 1_000,
        })
    }

    fn needs_owner(self) -> bool {
        matches!(self, Self::Set { .. })
    }

    fn is_omit(self) -> bool {
        matches!(self, Self::Omit)
    }
}

fn requested_times_from_timespecs(
    times: *const [LinuxTimespec; 2],
) -> Result<[RequestedTime; 2], SyscallError> {
    if times.is_null() {
        return Ok([RequestedTime::Now, RequestedTime::Now]);
    }

    let times = user_safe::read(times)?;
    Ok([
        RequestedTime::from_timespec(times[0])?,
        RequestedTime::from_timespec(times[1])?,
    ])
}

struct UtimeCredentials {
    uid: u32,
    gid: u32,
    supplementary_groups: Vec<u32>,
}

fn current_credentials() -> UtimeCredentials {
    let process = get_current_process();
    let process = process.lock();
    UtimeCredentials {
        uid: process.effective_uid,
        gid: process.effective_gid,
        supplementary_groups: process.supplementary_groups.clone(),
    }
}

fn check_write_permission(info: &FileLikeInfo, credentials: &UtimeCredentials) -> bool {
    if credentials.uid == 0 {
        return true;
    }

    let shift = if credentials.uid == info.uid {
        6
    } else if credentials.gid == info.gid || credentials.supplementary_groups.contains(&info.gid) {
        3
    } else {
        0
    };
    ((info.permission.0 >> shift) & 0o2) != 0
}

fn build_file_times(current: FileTimes, requested: [RequestedTime; 2]) -> FileTimes {
    let now = FileTimes::now();
    let (atime_sec, atime_nsec) = match requested[0] {
        RequestedTime::Now => (now.atime_sec, now.atime_nsec),
        RequestedTime::Omit => (current.atime_sec, current.atime_nsec),
        RequestedTime::Set { sec, nsec } => (sec, nsec),
    };
    let (mtime_sec, mtime_nsec) = match requested[1] {
        RequestedTime::Now => (now.mtime_sec, now.mtime_nsec),
        RequestedTime::Omit => (current.mtime_sec, current.mtime_nsec),
        RequestedTime::Set { sec, nsec } => (sec, nsec),
    };

    FileTimes::from_parts(
        atime_sec,
        atime_nsec,
        mtime_sec,
        mtime_nsec,
        now.ctime_sec,
        now.ctime_nsec,
    )
}

fn check_utime_permissions(
    info: &FileLikeInfo,
    requested: [RequestedTime; 2],
) -> Result<(), SyscallError> {
    let credentials = current_credentials();
    if credentials.uid == 0 || credentials.uid == info.uid {
        return Ok(());
    }

    if requested.iter().all(|time| !time.needs_owner()) {
        if check_write_permission(info, &credentials) {
            Ok(())
        } else {
            Err(SyscallError::AccessDenied)
        }
    } else {
        Err(SyscallError::PermissionDenied)
    }
}

fn set_times_on_path(
    path: Path,
    requested: [RequestedTime; 2],
    follow_symlink: bool,
) -> Result<usize, SyscallError> {
    let object = if follow_symlink {
        open_path(path.clone())?
    } else {
        open_path_nofollow(path.clone())?
    };
    if requested.iter().all(|time| time.is_omit()) {
        return Ok(0);
    }

    VirtualFS
        .lock()
        .ensure_writable_mount(path.clone())
        .map_err(SyscallError::from)?;
    let info = object.info()?;
    check_utime_permissions(&info, requested)?;
    let times = build_file_times(info.times, requested);
    object
        .set_times(times, follow_symlink)
        .map_err(SyscallError::from)?;
    Ok(0)
}

fn set_times_on_fd(fd: i32, requested: [RequestedTime; 2]) -> Result<usize, SyscallError> {
    let object = get_object_current_process(fd as u64).map_err(SyscallError::from)?;
    let file_like = object.as_file_like()?;
    if requested.iter().all(|time| time.is_omit()) {
        return Ok(0);
    }

    VirtualFS
        .lock()
        .ensure_writable_mount(file_like.path())
        .map_err(SyscallError::from)?;
    let info = file_like.info()?;
    check_utime_permissions(&info, requested)?;
    let times = build_file_times(info.times, requested);
    file_like
        .set_times(times, true)
        .map_err(SyscallError::from)?;
    Ok(0)
}

define_syscall!(Utime, |path: CString, times: *const LinuxUtimbuf| {
    let path = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path)?;
    let requested = if times.is_null() {
        [RequestedTime::Now, RequestedTime::Now]
    } else {
        let times = user_safe::read(times)?;
        [
            RequestedTime::from_seconds(times.actime)?,
            RequestedTime::from_seconds(times.modtime)?,
        ]
    };
    set_times_on_path(path, requested, true)
});

define_syscall!(Utimes, |path: CString, times: *const [LinuxTimeval; 2]| {
    let path = path_from_raw(path)?;
    let path = resolve_path_at(AT_FDCWD, &path)?;
    let requested = if times.is_null() {
        [RequestedTime::Now, RequestedTime::Now]
    } else {
        let times = user_safe::read(times)?;
        [
            RequestedTime::from_timeval(times[0])?,
            RequestedTime::from_timeval(times[1])?,
        ]
    };
    set_times_on_path(path, requested, true)
});

define_syscall!(Utimensat, |dirfd: i32,
                            path: u64,
                            times: *const [LinuxTimespec; 2],
                            flags: AtFlags| {
    let allowed_flags = AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH;
    if flags.bits() != (flags & allowed_flags).bits() {
        return Err(SyscallError::InvalidArguments);
    }

    let requested = requested_times_from_timespecs(times)?;
    let path = path as CString;
    if path.is_null() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            return Err(SyscallError::InvalidArguments);
        }
        if dirfd >= 0 {
            return set_times_on_fd(dirfd, requested);
        }
        return Err(SyscallError::BadAddress);
    }

    let path_str = path_from_raw(path)?;
    if path_str.is_empty() {
        if flags.contains(AtFlags::EMPTY_PATH) {
            return set_times_on_fd(dirfd, requested);
        }
        return Err(SyscallError::FileNotFound);
    }

    let path = resolve_path_at(dirfd, &path_str)?;
    set_times_on_path(path, requested, !flags.contains(AtFlags::SYMLINK_NOFOLLOW))
});
