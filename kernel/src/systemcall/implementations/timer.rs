use crate::{
    misc::timer::{ClockId, Sigevent, TimerNotifyMethod, TimerSpec, TimerState},
    process::misc::with_current_process,
    systemcall::utils::{SyscallError, SyscallImpl, SyscallResult},
};

use crate::define_syscall;

define_syscall!(
    TimerCreate,
    |time_type: ClockId, notify_method: *const Sigevent| {
        unsafe {
            with_current_process(|process| {
                Ok(process.create_timer(time_type, TimerNotifyMethod::from(*notify_method)))
            })
        }
    }
);

define_syscall!(TimerDelete, |id: usize| {
    with_current_process(|process| process.delete_timer(id))?;
    Ok(0)
});

define_syscall!(TimerGetoverrun, |id: usize| {
    with_current_process(|process| process.get_timer_overrun(id))
});

define_syscall!(TimerSettime, |id: usize, timer_state: *const TimerSpec| {
    unsafe {
        with_current_process(|process| {
            process
                .timers
                .get_mut(id)
                .ok_or(SyscallError::InvalidArguments)?
                .as_mut()
                .ok_or(SyscallError::InvalidArguments)?
                .state = TimerState::from(*timer_state);
            Ok(0)
        })
    }
});

define_syscall!(TimerGettime, |id: usize, timer_state: *mut TimerSpec| {
    unsafe {
        with_current_process(|process| {
            *timer_state = TimerSpec::from(
                process
                    .timers
                    .get_mut(id)
                    .ok_or(SyscallError::InvalidArguments)?
                    .as_mut()
                    .ok_or(SyscallError::InvalidArguments)?
                    .state,
            );
            Ok(0)
        })
    }
});
