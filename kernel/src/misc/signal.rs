use alloc::vec::Vec;
use crate::process::Process;

pub use seele_sys::signal::{SIGNAL_AMOUNT, Signal, SignalHandlerFn};

pub mod action {
    pub use seele_sys::signal::{SignalAction, SignalHandlingType, Signals};
}

pub fn default_signal_action_vec() -> Vec<action::SignalAction> {
    alloc::vec![action::SignalAction::default(); SIGNAL_AMOUNT]
}

pub mod misc {
    pub use super::default_signal_action_vec;
}

impl Process {
    pub fn get_signal_action(&mut self, signal: Signal) -> &mut action::SignalAction {
        &mut self.signal_actions[signal as usize]
    }
}
