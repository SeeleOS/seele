use alloc::vec::Vec;

use crate::signal::{Signal, SignalHandlerFn};

/// The action that a process will take when it got a signal
pub struct SignalAction {
    pub handler: SignalHandler,
    pub ignored_signals: Vec<Signal>,
}

pub enum SignalHandler {
    Default,
    Ignore,
    Function(SignalHandlerFn),
}
