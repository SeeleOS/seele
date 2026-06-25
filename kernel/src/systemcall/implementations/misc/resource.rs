use super::*;

const RLIM_INFINITY: u64 = u64::MAX;
const DEFAULT_RLIMIT_SIGPENDING: u64 = 4096;
const DEFAULT_RLIMIT_MSGQUEUE: u64 = 819_200;
const DEFAULT_RLIMIT_RTTIME: u64 = RLIM_INFINITY;

fn get_rlimit(resource: RlimitResource) -> LinuxRlimit64 {
    let process = get_current_process();
    let process = process.lock();
    match resource {
        RlimitResource::Cpu
        | RlimitResource::Rss
        | RlimitResource::As
        | RlimitResource::Locks
        | RlimitResource::Nice => LinuxRlimit64 {
            rlim_cur: RLIM_INFINITY,
            rlim_max: RLIM_INFINITY,
        },
        RlimitResource::Data => LinuxRlimit64 {
            rlim_cur: process.rlimit_data_cur,
            rlim_max: process.rlimit_data_max,
        },
        RlimitResource::Core => LinuxRlimit64 {
            rlim_cur: process.rlimit_core_cur,
            rlim_max: process.rlimit_core_max,
        },
        RlimitResource::Fsize => LinuxRlimit64 {
            rlim_cur: process.rlimit_fsize_cur,
            rlim_max: process.rlimit_fsize_max,
        },
        RlimitResource::Nproc => LinuxRlimit64 {
            rlim_cur: process.rlimit_nproc_cur,
            rlim_max: process.rlimit_nproc_max,
        },
        RlimitResource::Stack => LinuxRlimit64 {
            rlim_cur: process.rlimit_stack_cur,
            rlim_max: process.rlimit_stack_max,
        },
        RlimitResource::NoFile => LinuxRlimit64 {
            rlim_cur: process.rlimit_nofile_cur,
            rlim_max: process.rlimit_nofile_max,
        },
        RlimitResource::MemLock => LinuxRlimit64 {
            rlim_cur: process.rlimit_memlock_cur,
            rlim_max: process.rlimit_memlock_max,
        },
        RlimitResource::RtPrio => LinuxRlimit64 {
            rlim_cur: process.rlimit_rtprio_cur,
            rlim_max: process.rlimit_rtprio_max,
        },
        RlimitResource::Sigpending => LinuxRlimit64 {
            rlim_cur: DEFAULT_RLIMIT_SIGPENDING,
            rlim_max: DEFAULT_RLIMIT_SIGPENDING,
        },
        RlimitResource::Msgqueue => LinuxRlimit64 {
            rlim_cur: DEFAULT_RLIMIT_MSGQUEUE,
            rlim_max: DEFAULT_RLIMIT_MSGQUEUE,
        },
        RlimitResource::Rttime => LinuxRlimit64 {
            rlim_cur: DEFAULT_RLIMIT_RTTIME,
            rlim_max: DEFAULT_RLIMIT_RTTIME,
        },
    }
}

fn set_rlimit(resource: RlimitResource, limit: LinuxRlimit64) {
    let process = get_current_process();
    let mut process = process.lock();
    match resource {
        RlimitResource::Cpu
        | RlimitResource::Rss
        | RlimitResource::As
        | RlimitResource::Locks
        | RlimitResource::Sigpending
        | RlimitResource::Msgqueue
        | RlimitResource::Nice
        | RlimitResource::Rttime => {}
        RlimitResource::Core => {
            process.rlimit_core_cur = limit.rlim_cur;
            process.rlimit_core_max = limit.rlim_max;
        }
        RlimitResource::Fsize => {
            process.rlimit_fsize_cur = limit.rlim_cur;
            process.rlimit_fsize_max = limit.rlim_max;
        }
        RlimitResource::Data => {
            process.rlimit_data_cur = limit.rlim_cur;
            process.rlimit_data_max = limit.rlim_max;
        }
        RlimitResource::Nproc => {
            process.rlimit_nproc_cur = limit.rlim_cur;
            process.rlimit_nproc_max = limit.rlim_max;
        }
        RlimitResource::Stack => {
            process.rlimit_stack_cur = limit.rlim_cur;
            process.rlimit_stack_max = limit.rlim_max;
        }
        RlimitResource::NoFile => {
            process.rlimit_nofile_cur = limit.rlim_cur;
            process.rlimit_nofile_max = limit.rlim_max;
        }
        RlimitResource::MemLock => {
            process.rlimit_memlock_cur = limit.rlim_cur;
            process.rlimit_memlock_max = limit.rlim_max;
        }
        RlimitResource::RtPrio => {
            process.rlimit_rtprio_cur = limit.rlim_cur;
            process.rlimit_rtprio_max = limit.rlim_max;
        }
    }
}

define_syscall!(Getrlimit, |resource: i32, rlimit: *mut LinuxRlimit64| {
    let resource =
        RlimitResource::try_from(resource as u32).map_err(|_| SyscallError::InvalidArguments)?;
    user_safe::write(rlimit, &get_rlimit(resource))?;
    Ok(0)
});

define_syscall!(Setrlimit, |resource: i32, rlimit: u64| {
    let resource =
        RlimitResource::try_from(resource as u32).map_err(|_| SyscallError::InvalidArguments)?;
    let limit = user_safe::read(rlimit as *const LinuxRlimit64)?;
    set_rlimit(resource, limit);
    Ok(0)
});

define_syscall!(
    Prlimit64,
    |pid: i32, resource: u32, new_limit: *const LinuxRlimit64, old_limit: *mut LinuxRlimit64| {
        if pid != 0 {
            return Err(SyscallError::InvalidArguments);
        }

        let resource =
            RlimitResource::try_from(resource).map_err(|_| SyscallError::InvalidArguments)?;
        let new_limit_value = if new_limit.is_null() {
            None
        } else {
            Some(user_safe::read(new_limit)?)
        };

        if !old_limit.is_null() {
            user_safe::write(old_limit, &get_rlimit(resource))?;
        }

        if let Some(limit) = new_limit_value {
            set_rlimit(resource, limit);
        }

        Ok(0)
    }
);
