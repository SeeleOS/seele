use alloc::vec::Vec;

use crate::signal::{Signal, SignalHandlerFn};

/// The action that a process will take when it got a signal
#[derive(Default, Clone, Debug)]
pub struct SignalAction {
    pub handler: SignalHandler,
    // Signals which the process will ignore when its in the signal handler
    pub ignored_signals: Vec<Signal>,
}

#[derive(Default, Clone, Debug)]
pub enum SignalHandler {
    #[default]
    Default,
    Ignore,
    Function(SignalHandlerFn),
}
