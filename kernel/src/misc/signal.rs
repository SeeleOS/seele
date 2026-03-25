use crate::{
    process::{
        Process,
        manager::{MANAGER, get_current_process},
    },
};
use alloc::vec::Vec;

pub use seele_sys::signal::{SIGNAL_AMOUNT, Signal, SignalHandlerFn};
use seele_sys::signal::{SignalHandlingType, Signals};
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
            if self.pending_signals.contains(signal_bits) {
                let handling_type = self.signal_actions[signal as usize].handling_type.clone();
                self.pending_signals.remove(signal_bits);

                match handling_type {
                    SignalHandlingType::Default => signal.default_action(),
                    SignalHandlingType::Ignore => {}
                    SignalHandlingType::Function(_) => todo!(),
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
