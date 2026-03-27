use crate::{
    process::{
        Process,
        manager::{MANAGER, get_current_process},
    },
    thread::{get_current_thread, misc::SnapshotState, snapshot::ThreadSnapshot},
};
use alloc::vec::Vec;

pub use seele_sys::signal::{SIGNAL_AMOUNT, Signal, SignalHandlerFn};
use seele_sys::{
    permission::Permissions,
    signal::{SignalHandlingType, Signals},
};
use strum::IntoEnumIterator;

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

    pub fn send_signal(&mut self, signal: Signal) {
        self.pending_signals.insert(Signals::from(signal));
    }

    pub fn process_signals(&mut self) {
        for signal in Signal::iter() {
            let signal_bits = Signals::from(signal);
            if self.pending_signals.contains(signal_bits)
                && !get_current_thread()
                    .lock()
                    .blocked_signals
                    .contains(signal_bits)
            {
                let handling_type = self.signal_actions[signal as usize].handling_type.clone();
                self.pending_signals.remove(signal_bits);

                match handling_type {
                    SignalHandlingType::Default => signal.default_action(),
                    SignalHandlingType::Ignore => {}
                    SignalHandlingType::Function(func) => {
                        let current_thread_ref = get_current_thread();
                        let mut current_thread = current_thread_ref.lock();

                        let current_proc = get_current_process();
                        let mut current_proc = current_proc.lock();

                        let stack = current_proc
                            .addrspace
                            .allocate_user_lazy(16, Permissions::all())
                            .as_u64();

                        current_thread.snapshot_state = SnapshotState::SignalHandler;
                        current_thread.sig_handler_snapshot = ThreadSnapshot::new(
                            func as u64,
                            &mut current_proc.addrspace,
                            stack,
                            crate::thread::snapshot::ThreadSnapshotType::Thread,
                        )
                    }
                }
            }
        }
    }
}

pub trait SignalExtension {
    fn default_action(&self);
}

impl SignalExtension for Signal {
    fn default_action(&self) {
        match self {
            Self::Terminate
            | Self::Kill
            | Self::Interrupt
            | Self::Quit
            | Self::Abort
            | Self::InvalidMemoryAccess
            | Self::BrokenPipe
            | Self::Hangup
            | Self::FloatingPointError
            | Self::IllegalInstruction
            | Self::Trap => {
                let current = get_current_process();
                MANAGER.lock().destroy_process(current);
            }
            Self::ChildChanged => {}
            Self::Stop => todo!(),
            Self::Continue => {}
            Self::Alarm => {}
            Self::TerminalStop => todo!(),
        }
    }
}
