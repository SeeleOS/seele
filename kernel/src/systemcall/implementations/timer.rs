use crate::{
    misc::timer::{TimerNotifyMethod, TimerState},
    process::misc::with_current_process,
    systemcall::utils::{SyscallError, SyscallImpl, SyscallResult},
};
use seele_sys::abi::time::{TimeType, TimerNotifyStruct, TimerStateStruct};

use crate::define_syscall;

define_syscall!(
    TimerCreate,
    |time_type: TimeType, notify_method: *const TimerNotifyStruct| {
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

define_syscall!(
    TimerSettime,
    |id: usize, timer_state: *const TimerStateStruct| {
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
    }
);

define_syscall!(
    TimerGettime,
    |id: usize, timer_state: *mut TimerStateStruct| {
        unsafe {
            with_current_process(|process| {
                *timer_state = TimerStateStruct::from(
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
    }
);
