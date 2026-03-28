use crate::{
    misc::snapshot::Snapshot,
    process::{
        Process,
        manager::{MANAGER, get_current_process},
        misc::with_current_process,
    },
    thread::{
        get_current_thread,
        misc::{SnapshotState, with_current_thread},
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        thread::Thread,
    },
};
use alloc::vec::Vec;

pub use seele_sys::signal::{SIGNAL_AMOUNT, SigHandlerFn2, Signal, SignalHandlerFn};
use seele_sys::signal::{SigInfo, SignalHandlingType, Signals, UContext};
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
                let action = self.signal_actions[signal as usize].clone();
                self.pending_signals.remove(signal_bits);

                match action.handling_type {
                    SignalHandlingType::Default => signal.default_action(),
                    SignalHandlingType::Ignore => {}
                    SignalHandlingType::Function1(func) => {
                        with_current_process(|current_proc| {
                            with_current_thread(|current_thread| {
                                let (_, mut stack_builder) =
                                    current_proc.addrspace.allocate_user(16);
                                stack_builder.push(action.restorer as u64);

                                let mut thread_snapshot = ThreadSnapshot::new(
                                    func as u64,
                                    &mut current_proc.addrspace,
                                    stack_builder.finish().as_u64(),
                                    ThreadSnapshotType::Thread,
                                );

                                thread_snapshot.inner.rdi = signal as u64;

                                current_thread.block_signals_for_handler(
                                    action.sig_handler_ignored_sigs,
                                    signal,
                                );
                                current_thread.enter_signal_handler(thread_snapshot);
                            })
                        });
                    }
                    SignalHandlingType::Function2(func) => {
                        with_current_process(|current_proc| {
                            with_current_thread(|current_thread| {
                                let (_, mut stack_builder) =
                                    current_proc.addrspace.allocate_user(16);
                                let (_, mut frame_builder) =
                                    current_proc.addrspace.allocate_user(1);

                                let siginfo = SigInfo {
                                    si_signo: signal as i32,
                                    ..Default::default()
                                };
                                let ucontext = build_signal_ucontext(&current_thread);

                                let ucontext_ptr = frame_builder.push_struct(&ucontext);
                                let siginfo_ptr = frame_builder.push_struct(&siginfo);

                                stack_builder.push(action.restorer as u64);

                                let mut thread_snapshot = ThreadSnapshot::new(
                                    func as u64,
                                    &mut current_proc.addrspace,
                                    stack_builder.finish().as_u64(),
                                    ThreadSnapshotType::Thread,
                                );

                                thread_snapshot.inner.rdi = signal as u64;
                                thread_snapshot.inner.rsi = siginfo_ptr;
                                thread_snapshot.inner.rdx = ucontext_ptr;

                                current_thread.block_signals_for_handler(
                                    action.sig_handler_ignored_sigs,
                                    signal,
                                );
                                current_thread.enter_signal_handler(thread_snapshot);
                            })
                        });
                    }
                }
            }
        }
    }
}

impl Thread {
    fn block_signals_for_handler(&mut self, mut signals_to_block: Signals, signal: Signal) {
        signals_to_block.insert(Signals::from(signal));
        self.saved_blocked_signals.push(self.blocked_signals);
        self.blocked_signals.insert(signals_to_block);
    }

    fn enter_signal_handler(&mut self, snapshot: ThreadSnapshot) {
        self.snapshot_state = SnapshotState::SignalHandler;
        self.sig_handler_snapshot = snapshot;
    }

    pub fn restore_blocked_signals(&mut self) {
        if let Some(mask) = self.saved_blocked_signals.pop() {
            self.blocked_signals = mask;
        }
    }
}

fn build_signal_ucontext(thread: &Thread) -> UContext {
    let snapshot = match thread.snapshot_state {
        SnapshotState::Normal => &thread.snapshot.inner,
        SnapshotState::SignalHandler => &thread.sig_handler_snapshot.inner,
    };

    UContext {
        blocked_signals: thread.blocked_signals.bits(),
        gregs: snapshot_to_gregs(snapshot),
    }
}

fn snapshot_to_gregs(snapshot: &Snapshot) -> [u64; 20] {
    [
        snapshot.r15,
        snapshot.r14,
        snapshot.r13,
        snapshot.r12,
        snapshot.r11,
        snapshot.r10,
        snapshot.r9,
        snapshot.r8,
        snapshot.rdi,
        snapshot.rsi,
        snapshot.rbp,
        snapshot.rbx,
        snapshot.rdx,
        snapshot.rcx,
        snapshot.rax as u64,
        snapshot.rip,
        snapshot.cs,
        snapshot.rflags,
        snapshot.rsp,
        snapshot.ss,
    ]
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
