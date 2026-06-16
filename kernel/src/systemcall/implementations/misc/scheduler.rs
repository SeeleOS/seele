use super::*;

define_syscall!(SchedYield, { Ok(0) });

define_syscall!(Madvise, |_addr: u64, _len: usize, _advice: i32| { Ok(0) });

define_syscall!(Getpriority, |_which: i32, _who: i32| { Ok(0) });

define_syscall!(Setpriority, |_which: i32, _who: i32, _prio: i32| { Ok(0) });

define_syscall!(IoprioSet, |which: LinuxIoprioWho, who: i32, ioprio: i32| {
    let (class, _level) = decode_linux_ioprio(ioprio)?;
    validate_linux_ioprio_target(which, who)?;
    if matches!(class, LinuxIoprioClass::None) {
        return Ok(0);
    }
    Ok(0)
});

define_syscall!(IoprioGet, |which: LinuxIoprioWho, who: i32| {
    validate_linux_ioprio_target(which, who)?;
    Ok(default_linux_ioprio())
});

define_syscall!(SchedSetparam, |pid: i32, param: *const LinuxSchedParam| {
    if pid < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if param.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let param = user_safe::read(param)?;
    if param.sched_priority < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(0)
});

define_syscall!(SchedGetparam, |pid: i32, param: *mut LinuxSchedParam| {
    if pid < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if param.is_null() {
        return Err(SyscallError::BadAddress);
    }

    user_safe::write(param, &LinuxSchedParam { sched_priority: 0 })?;
    Ok(0)
});

define_syscall!(
    SchedSetscheduler,
    |pid: i32, policy: LinuxSchedPolicy, param: *const LinuxSchedParam| {
        if pid < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if param.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let param = user_safe::read(param)?;
        if param.sched_priority < policy.min_priority()
            || param.sched_priority > policy.max_priority()
        {
            return Err(SyscallError::InvalidArguments);
        }

        Ok(0)
    }
);

#[cfg(test)]
mod tests {
    use crate::systemcall::test::*;

    crate::test!(
        scheduler_priority_and_io_permission_syscalls,
        "scheduler priority and io permission syscalls validate linux arguments",
        scheduler_priority_and_io_permission_syscalls_validate_linux_arguments
    );
}

define_syscall!(SchedGetscheduler, |pid: i32| {
    if pid < 0 {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(LinuxSchedPolicy::Other as usize)
});

define_syscall!(SchedGetPriorityMax, |policy: LinuxSchedPolicy| {
    Ok(policy.max_priority() as usize)
});

define_syscall!(SchedGetPriorityMin, |policy: LinuxSchedPolicy| {
    Ok(policy.min_priority() as usize)
});

define_syscall!(Iopl, |level: i32| {
    if !(0..=3).contains(&level) {
        return Err(SyscallError::InvalidArguments);
    }

    Ok(0)
});

define_syscall!(Ioperm, |_from: u64, _num: u64, _turn_on: i32| { Ok(0) });
define_syscall!(
    SchedSetaffinity,
    |pid: i32, cpusetsize: usize, mask_ptr: *const u8| {
        if pid < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if cpusetsize == 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if mask_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }

        Ok(0)
    }
);

define_syscall!(
    SchedGetaffinity,
    |pid: i32, cpusetsize: usize, mask_ptr: *mut u8| {
        if pid < 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if cpusetsize < core::mem::size_of::<usize>() {
            return Err(SyscallError::InvalidArguments);
        }

        let mut mask = vec![0; cpusetsize];
        mask[0] = 1;
        user_safe::write_buffer(mask_ptr, &mask)?;

        Ok(core::mem::size_of::<usize>())
    }
);
