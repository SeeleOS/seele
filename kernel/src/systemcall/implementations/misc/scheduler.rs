use super::*;

define_syscall!(SchedYield, {
    let current = crate::thread::get_current_thread();
    if alloc::sync::Arc::ptr_eq(&current, &crate::thread::scheduler_thread()) {
        return Ok(0);
    }

    return_to_scheduler_from_current();
    Ok(0)
});

define_syscall!(Madvise, |_addr: u64, _len: usize, _advice: i32| { Ok(0) });

define_syscall!(Getpriority, |which: i32, who: i32| {
    match which {
        0..=2 => {}
        _ => return Err(SyscallError::InvalidArguments),
    }
    if who < 0 {
        return Err(SyscallError::NoProcess);
    }

    Ok(20)
});

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

pub fn sched_process_for_pid(pid: i32) -> Result<ProcessRef, SyscallError> {
    if pid < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    if pid == 0 {
        Ok(get_current_process())
    } else {
        get_process_with_pid(ProcessID(pid as u64))
    }
}

define_syscall!(SchedSetparam, |pid: i32, param: *const LinuxSchedParam| {
    if param.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let process = sched_process_for_pid(pid)?;
    let param = user_safe::read(param)?;
    if param.sched_priority < 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let mut process = process.lock();
    if param.sched_priority < process.sched_policy.min_priority()
        || param.sched_priority > process.sched_policy.max_priority()
    {
        return Err(SyscallError::InvalidArguments);
    }
    process.sched_priority = param.sched_priority;

    Ok(0)
});

define_syscall!(SchedGetparam, |pid: i32, param: *mut LinuxSchedParam| {
    if param.is_null() {
        return Err(SyscallError::InvalidArguments);
    }

    let process = sched_process_for_pid(pid)?;
    let sched_priority = process.lock().sched_priority;
    user_safe::write(param, &LinuxSchedParam { sched_priority })?;
    Ok(0)
});

define_syscall!(
    SchedSetscheduler,
    |pid: i32, policy: LinuxSchedPolicy, param: *const LinuxSchedParam| {
        if param.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let process = sched_process_for_pid(pid)?;
        let param = user_safe::read(param)?;
        if param.sched_priority < policy.min_priority()
            || param.sched_priority > policy.max_priority()
        {
            return Err(SyscallError::InvalidArguments);
        }
        let mut process = process.lock();
        process.sched_policy = policy;
        process.sched_priority = param.sched_priority;

        Ok(0)
    }
);

#[cfg(test)]
mod tests {
    use crate::systemcall::{
        implementations::{
            Getcpu, Getpriority, Ioperm, Iopl, IoprioGet, IoprioSet, Madvise, SchedGetPriorityMax,
            SchedGetPriorityMin, SchedGetscheduler, SchedYield, Setpriority,
        },
        test_helpers::{
            SyscallArgs, allocate_user_test_page, expect_errno, expect_ok, read_user_value,
        },
        utils::SyscallError,
    };

    crate::test!(
        scheduler_priority_and_io_permission_syscalls,
        "scheduler priority and io permission syscalls validate linux arguments",
        scheduler_priority_and_io_permission_syscalls_validate_linux_arguments
    );

    fn scheduler_priority_and_io_permission_syscalls_validate_linux_arguments() {
        expect_ok(SyscallArgs::none().call::<SchedYield>(), 0);
        expect_ok(SyscallArgs::new([0, 4096, 0, 0, 0, 0]).call::<Madvise>(), 0);
        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getpriority>(),
            20,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<Getpriority>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, u64::MAX, 0, 0, 0, 0]).call::<Getpriority>(),
            SyscallError::NoProcess,
        );
        expect_ok(
            SyscallArgs::new([0, 0, 10, 0, 0, 0]).call::<Setpriority>(),
            0,
        );

        expect_ok(
            SyscallArgs::new([1, 0, (2u64 << 13) | 4, 0, 0, 0]).call::<IoprioSet>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<IoprioGet>(),
            2usize << 13,
        );
        expect_errno(
            SyscallArgs::new([1, u64::MAX, 0, 0, 0, 0]).call::<IoprioGet>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([1, 0, (2u64 << 13) | 8, 0, 0, 0]).call::<IoprioSet>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([99, 0, 0, 0, 0, 0]).call::<IoprioGet>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedGetscheduler>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<SchedGetscheduler>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMin>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMin>(),
            1,
        );
        expect_ok(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMax>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMax>(),
            99,
        );
        expect_errno(
            SyscallArgs::new([99, 0, 0, 0, 0, 0]).call::<SchedGetPriorityMax>(),
            SyscallError::InvalidArguments,
        );

        expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Iopl>(), 0);
        expect_ok(SyscallArgs::new([3, 0, 0, 0, 0, 0]).call::<Iopl>(), 0);
        expect_errno(
            SyscallArgs::new([4, 0, 0, 0, 0, 0]).call::<Iopl>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(SyscallArgs::new([0, 1, 1, 0, 0, 0]).call::<Ioperm>(), 0);

        let page = allocate_user_test_page();
        expect_ok(
            SyscallArgs::new([page, page + 4, 0, 0, 0, 0]).call::<Getcpu>(),
            0,
        );
        assert_eq!(read_user_value::<u32>(page), 0);
        assert_eq!(read_user_value::<u32>(page + 4), 0);
        expect_ok(SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Getcpu>(), 0);
        expect_errno(
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<Getcpu>(),
            SyscallError::BadAddress,
        );
    }
}

define_syscall!(SchedGetscheduler, |pid: i32| {
    let process = sched_process_for_pid(pid)?;
    Ok(process.lock().sched_policy as usize)
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
        if cpusetsize == 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if mask_ptr.is_null() {
            return Err(SyscallError::BadAddress);
        }
        sched_process_for_pid(pid)?;

        Ok(0)
    }
);

define_syscall!(
    SchedGetaffinity,
    |pid: i32, cpusetsize: usize, mask_ptr: *mut u8| {
        if cpusetsize < core::mem::size_of::<usize>() {
            return Err(SyscallError::InvalidArguments);
        }
        sched_process_for_pid(pid)?;

        let mut mask = vec![0; cpusetsize];
        mask[0] = 1;
        user_safe::write_buffer(mask_ptr, &mask)?;

        Ok(core::mem::size_of::<usize>())
    }
);

define_syscall!(Getcpu, |cpu: *mut u32, node: *mut u32, _tcache: *mut u8| {
    if !cpu.is_null() {
        user_safe::write(cpu, &0u32)?;
    }
    if !node.is_null() {
        user_safe::write(node, &0u32)?;
    }

    Ok(0)
});
