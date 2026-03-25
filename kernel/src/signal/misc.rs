use alloc::vec::Vec;
use seele_sys::signal::Signal;

use crate::{
    process::Process,
    signal::{SIGNAL_AMOUNT, action::SignalAction},
};

pub fn default_signal_action_vec() -> Vec<SignalAction> {
    alloc::vec![SignalAction::default(); SIGNAL_AMOUNT]
}

impl Process {
    pub fn get_signal_action(&mut self, signal: Signal) -> &mut SignalAction {
        &mut self.signal_actions[signal as usize]
    }
}
