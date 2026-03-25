use alloc::vec::Vec;

use crate::signal::{Signal, SignalHandlerFn};

/// The action that a process will take when it got a signal
#[derive(Default, Clone, Debug)]
#[repr(C)]
pub struct SignalAction {
    pub handling_type: SignalHandlingType,
    // Signals which the process will ignore when its in the signal handler
    pub ignored_signals: Vec<Signal>,
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
