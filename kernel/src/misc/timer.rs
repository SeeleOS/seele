use seele_sys::{
    SyscallResult,
    abi::time::{TimeType, TimerNotifyMethod},
    errors::SyscallError,
};

use crate::{
    misc::{others::push_and_return_index, time::Time},
    process::Process,
};

#[derive(Debug, Clone, Copy)]
pub enum TimerState {
    Disabled,
    OneShot { deadline: Time },
    Periodic { deadline: Time, interval: Time },
}

#[derive(Debug)]
pub struct Timer {
    pub notify_method: TimerNotifyMethod,
    pub time_type: TimeType,
    pub state: TimerState,
}

impl Timer {
    pub fn get_appropriate_time(&self) -> Time {
        match self.time_type {
            TimeType::Realtime => Time::current(),
            TimeType::SinceBoot => Time::since_boot(),
        }
    }

    pub fn process(&mut self) {
        if !self.is_over_deadline() {
            return;
        }

        self.state = match self.state {
            TimerState::Disabled => TimerState::Disabled,
            TimerState::OneShot { .. } => TimerState::Disabled,
            TimerState::Periodic { deadline, interval } => TimerState::Periodic {
                deadline: deadline.add_ns(interval.as_nanoseconds()),
                interval,
            },
        };
    }

    pub fn is_over_deadline(&self) -> bool {
        match self.state {
            TimerState::Disabled => false,
            TimerState::OneShot { deadline } | TimerState::Periodic { deadline, .. } => {
                deadline <= self.get_appropriate_time()
            }
        }
    }
}

impl Process {
    pub fn create_timer(
        &mut self,
        time_type: TimeType,
        notify_method: TimerNotifyMethod,
        state: TimerState,
    ) -> usize {
        let state = match state {
            TimerState::Disabled => TimerState::Disabled,
            TimerState::OneShot { deadline } => TimerState::OneShot { deadline },
            TimerState::Periodic { deadline, interval } if interval.as_nanoseconds() == 0 => {
                TimerState::OneShot { deadline }
            }
            TimerState::Periodic { deadline, interval } => {
                TimerState::Periodic { deadline, interval }
            }
        };

        let timer = Timer {
            notify_method,
            time_type,
            state,
        };

        push_and_return_index(&mut self.timers, Some(timer))
    }

    pub fn delete_timer(&mut self, index: usize) -> SyscallResult<()> {
        *self
            .timers
            .get_mut(index)
            .ok_or(SyscallError::InvalidArguments)? = None;

        Ok(())
    }

    pub fn process_timers(&mut self) {
        self.timers.iter_mut().flatten().for_each(Timer::process);
    }
}
