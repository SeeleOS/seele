use alloc::vec::Vec;

use crate::signal::{SIGNAL_AMOUNT, action::SignalAction};

pub fn default_signal_action_vec() -> Vec<SignalAction> {
    alloc::vec![SignalAction::default(); SIGNAL_AMOUNT]
}
