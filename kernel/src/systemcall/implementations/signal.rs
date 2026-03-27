use crate::process::ProcessRef;
use crate::process::manager::MANAGER;
use crate::process::misc::ProcessID;
use crate::signal::action::{SignalHandlingType, Signals};
use crate::systemcall::error::*;
use crate::systemcall::utils::*;
use crate::thread::get_current_thread;
use crate::thread::misc::SnapshotState;
use crate::thread::scheduling::return_to_executor_no_save;
use crate::{
    define_syscall,
    process::manager::get_current_process,
    signal::{Signal, action::SignalAction},
};

define_syscall!(
    RegisterSignalAction,
    |signal: Signal, new_action: *const SignalAction, old_action: *mut SignalAction| {
        let process = get_current_process();
        let mut process = process.lock();
        let current_signal_action = process.get_signal_action(signal);

        unsafe {
            if !old_action.is_null() {
                *old_action = current_signal_action.clone();
            }

            if !new_action.is_null() {
                *current_signal_action = (*new_action).clone();
            }
        }

        Ok(0)
    }
);

define_syscall!(SendSignal, |process: ProcessRef, signal: Signal| {
    process.lock().send_signal(signal);
    Ok(0)
});

define_syscall!(
    BlockSignals,
    |signals: Signals, old_signals: *mut Signals| {
        unsafe {
            *old_signals = get_current_thread().lock().blocked_signals;

            get_current_thread().lock().blocked_signals.insert(signals);
        }
        Ok(0)
    }
);

define_syscall!(
    UnblockSignals,
    |signals: Signals, old_signals: *mut Signals| {
        unsafe {
            *old_signals = get_current_thread().lock().blocked_signals;

            get_current_thread().lock().blocked_signals.remove(signals);
        }

        Ok(0)
    }
);

define_syscall!(
    SetBlockedSignals,
    |signals: Signals, old_signals: *mut Signals| {
        unsafe {
            *old_signals = get_current_thread().lock().blocked_signals;

            get_current_thread().lock().blocked_signals = signals;
        }

        Ok(0)
    }
);

define_syscall!(SigHandlerReturn, {
    get_current_thread().lock().snapshot_state = SnapshotState::Normal;
    get_current_thread().lock().restore_blocked_signals();

    return_to_executor_no_save();

    unreachable!()
});
