use alloc::vec::Vec;

use crate::signal::action::SignalHandlingType;
use crate::systemcall::error::*;
use crate::systemcall::numbers::*;
use crate::systemcall::utils::*;
use crate::{
    define_syscall,
    process::manager::get_current_process,
    signal::{self, Signal, action::SignalAction},
};
define_syscall!(RegisterSignalAction, |action: u64, signal: Signal| {
    get_current_process().lock().signal_actions[signal as usize] = SignalAction {
        handling_type: SignalHandlingType::from(action),
        ignored_signals: Vec::new(),
    };

    Ok(0)
});
