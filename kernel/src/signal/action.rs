use crate::signal::{Signal, SignalHandlerFn};
use bitflags::bitflags;

/// The action that a process will take when it got a signal
#[derive(Default, Clone, Debug)]
#[repr(C)]
pub struct SignalAction {
    pub handling_type: SignalHandlingType,
    // Signals which the process will ignore when its in the signal handler
    pub ignored_signals: Signals,
}

#[derive(Default, Clone, Debug)]
pub enum SignalHandlingType {
    #[default]
    Default,
    Ignore,
    Function(SignalHandlerFn),
}

impl From<u64> for SignalHandlingType {
    fn from(value: u64) -> Self {
        match value {
            0 => Self::Default,
            1 => Self::Ignore,
            _ => Self::Function(unsafe {
                core::mem::transmute::<usize, SignalHandlerFn>(value as usize)
            }),
        }
    }
}

bitflags! {
    #[derive(Default, Clone, Copy, Debug)]
    #[repr(transparent)]
    pub struct Signals: u64 {
        const TERMINATE = 1 << Signal::Terminate as u64;
        const KILL = 1 << Signal::Kill as u64;
        const INTERRUPT = 1 << Signal::Interrupt as u64;
    }
}

impl From<Signal> for Signals {
    fn from(value: Signal) -> Self {
        match value {
            Signal::Terminate => Self::TERMINATE,
            Signal::Kill => Self::KILL,
            Signal::Interrupt => Self::INTERRUPT,
        }
    }
}
