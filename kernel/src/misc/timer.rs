use alloc::vec::Vec;
use seele_sys::{
    SyscallResult,
    abi::time::{TimeType, TimerNotifyStruct, TimerStateStruct, TimerStateType},
    errors::SyscallError,
    signal::Signal,
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
pub enum TimerNotifyMethod {
    None,
    Signal(Signal),
}

impl From<TimerNotifyStruct> for TimerNotifyMethod {
    fn from(value: TimerNotifyStruct) -> Self {
        match value.notify_type {
            seele_sys::abi::time::TimerNotifyType::None => Self::None,
            seele_sys::abi::time::TimerNotifyType::Signal => Self::Signal(value.signal),
        }
    }
}

impl From<TimerStateStruct> for TimerState {
    fn from(value: TimerStateStruct) -> Self {
        match value.state_type {
            TimerStateType::Disabled => Self::Disabled,
            TimerStateType::OneShot => Self::OneShot {
                deadline: Time(value.deadline),
            },
            TimerStateType::Periodic => Self::Periodic {
                deadline: Time(value.deadline),
                interval: Time(value.interval),
            },
        }
    }
}

impl From<TimerState> for TimerStateStruct {
    fn from(value: TimerState) -> Self {
        match value {
            TimerState::Disabled => Self {
                state_type: TimerStateType::Disabled,
                deadline: 0,
                interval: 0,
            },
            TimerState::OneShot { deadline } => Self {
                state_type: TimerStateType::OneShot,
                deadline: deadline.0,
                interval: 0,
            },
            TimerState::Periodic { deadline, interval } => Self {
                state_type: TimerStateType::Periodic,
                deadline: deadline.0,
                interval: interval.0,
            },
        }
    }
}

#[derive(Debug)]
pub enum TimerAction {
    None,
    Signal(Signal),
}

#[derive(Debug)]
pub struct Timer {
    pub notify_method: TimerNotifyMethod,
    pub time_type: TimeType,
    pub state: TimerState,
    pub overrun: u64,
}

impl Timer {
    pub fn get_appropriate_time(&self) -> Time {
        match self.time_type {
            TimeType::Realtime => Time::current(),
            TimeType::SinceBoot => Time::since_boot(),
        }
    }

    pub fn process(&mut self) -> TimerAction {
        if !self.is_over_deadline() {
            return TimerAction::None;
        }

        let now = self.get_appropriate_time();
        self.overrun = 0;

        self.state = match self.state {
            TimerState::Disabled => TimerState::Disabled,
            TimerState::OneShot { .. } => TimerState::Disabled,
            TimerState::Periodic { deadline, interval } => {
                let interval_ns = interval.as_nanoseconds();
                let elapsed_ns = now.sub(deadline).as_nanoseconds();
                let expirations = elapsed_ns / interval_ns + 1;
                self.overrun = expirations.saturating_sub(1);

                TimerState::Periodic {
                    deadline: deadline.add_ns(interval_ns.saturating_mul(expirations)),
                    interval,
                }
            }
        };

        match self.notify_method {
            TimerNotifyMethod::Signal(signal) => TimerAction::Signal(signal),
            _ => TimerAction::None,
        }
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
    pub fn create_timer(&mut self, time_type: TimeType, notify_method: TimerNotifyMethod) -> usize {
        let timer = Timer {
            notify_method,
            time_type,
            state: TimerState::Disabled,
            overrun: 0,
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

    pub fn get_timer_overrun(&self, index: usize) -> SyscallResult<usize> {
        Ok(self
            .timers
            .get(index)
            .ok_or(SyscallError::InvalidArguments)?
            .as_ref()
            .ok_or(SyscallError::InvalidArguments)?
            .overrun
            .min(i32::MAX as u64) as usize)
    }

    pub fn process_timers(&mut self) {
        let mut actions: Vec<TimerAction> = Vec::new();

        for timer in self.timers.iter_mut().flatten() {
            actions.push(timer.process());
        }

        for action in actions {
            match action {
                TimerAction::None => {}
                TimerAction::Signal(signal) => self.send_signal(signal),
            }
        }
    }
}
