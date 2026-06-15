use super::*;

define_syscall!(Setrlimit, |resource: i32, rlimit: u64| {
    let resource =
        RlimitResource::try_from(resource as u32).map_err(|_| SyscallError::InvalidArguments)?;
    let limit = user_safe::read(rlimit as *const LinuxRlimit64)?;
    match resource {
        RlimitResource::Stack => {
            let process = get_current_process();
            let mut process = process.lock();
            process.rlimit_stack_cur = limit.rlim_cur;
            process.rlimit_stack_max = limit.rlim_max;
            Ok(0)
        }
        RlimitResource::NoFile => {
            let process = get_current_process();
            let mut process = process.lock();
            process.rlimit_nofile_cur = limit.rlim_cur;
            process.rlimit_nofile_max = limit.rlim_max;
            Ok(0)
        }
        RlimitResource::MemLock => {
            let process = get_current_process();
            let mut process = process.lock();
            process.rlimit_memlock_cur = limit.rlim_cur;
            process.rlimit_memlock_max = limit.rlim_max;
            Ok(0)
        }
        RlimitResource::RtPrio => {
            let process = get_current_process();
            let mut process = process.lock();
            process.rlimit_rtprio_cur = limit.rlim_cur;
            process.rlimit_rtprio_max = limit.rlim_max;
            Ok(0)
        }
    }
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
            let limit = {
                let process = get_current_process();
                let process = process.lock();
                match resource {
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
                }
            };
            user_safe::write(old_limit, &limit)?;
        }

        if let Some(limit) = new_limit_value {
            let process = get_current_process();
            let mut process = process.lock();
            match resource {
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

        Ok(0)
    }
);
