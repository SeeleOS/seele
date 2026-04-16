use crate::{
    misc::snapshot::Snapshot,
    process::Process,
    s_println,
    thread::{
        THREAD_MANAGER, get_current_thread,
        misc::{SnapshotState, State, with_current_thread},
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        thread::Thread,
        yielding::BlockType,
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
        &mut self.signal_actions[signal.index()]
    }

    pub fn send_signal(&mut self, signal: Signal) {
        match signal {
            Signal::Continue => {
                let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
                for weak in &self.threads {
                    let Some(thread) = weak.upgrade() else {
                        continue;
                    };

                    if matches!(thread.lock().state, State::Blocked(BlockType::Stopped)) {
                        thread_manager.wake(thread.clone());
                    }
                }
            }
            _ => {
                self.pending_signals.insert(Signals::from(signal));
                self.wake_blocked_threads();
            }
        }
    }

    /// Returns `true` if a user-space signal handler was installed and the
    /// caller should stop the current return path so the handler can run next.
    #[must_use]
    pub fn process_signals(&mut self) -> bool {
        let mut ret = false;

        for signal in Signal::iter() {
            let signal_bits = Signals::from(signal);
            if self.pending_signals.contains(signal_bits)
                && !get_current_thread()
                    .lock()
                    .blocked_signals
                    .contains(signal_bits)
            {
                let action = self.signal_actions[signal.index()].clone();
                self.pending_signals.remove(signal_bits);

                match action.handling_type {
                    SignalHandlingType::Default => {
                        if self.default_signal_action(signal) {
                            ret = true;
                        }
                    }
                    SignalHandlingType::Ignore => {}
                    SignalHandlingType::Function1(func) => with_current_thread(|current_thread| {
                        let (_, mut stack_builder) = self.addrspace.allocate_user(16);
                        // x86_64 SysV requires %rsp % 16 == 8 on function entry.
                        // We only push a single synthetic return address, so reserve one
                        // extra slot before it to keep the handler ABI-compliant.
                        stack_builder.push(0);
                        stack_builder.push(action.restorer as u64);

                        let mut thread_snapshot = ThreadSnapshot::new(
                            func as u64,
                            &mut self.addrspace,
                            stack_builder.finish().as_u64(),
                            ThreadSnapshotType::Thread,
                        );

                        thread_snapshot.inner.rdi = signal as u64;

                        current_thread
                            .block_signals_for_handler(action.sig_handler_ignored_sigs, signal);
                        current_thread.enter_signal_handler(thread_snapshot);

                        ret = true;
                    }),
                    SignalHandlingType::Function2(func) => with_current_thread(|current_thread| {
                        let (_, mut stack_builder) = self.addrspace.allocate_user(16);
                        let (_, mut frame_builder) = self.addrspace.allocate_user(1);

                        let siginfo = SigInfo {
                            si_signo: signal as i32,
                            ..Default::default()
                        };
                        let ucontext = build_signal_ucontext(&current_thread);

                        let ucontext_ptr = frame_builder.push_struct(&ucontext);
                        let siginfo_ptr = frame_builder.push_struct(&siginfo);

                        // Keep the signal handler entry stack ABI-aligned.
                        stack_builder.push(0);
                        stack_builder.push(action.restorer as u64);

                        let mut thread_snapshot = ThreadSnapshot::new(
                            func as u64,
                            &mut self.addrspace,
                            stack_builder.finish().as_u64(),
                            ThreadSnapshotType::Thread,
                        );

                        thread_snapshot.inner.rdi = signal as u64;
                        thread_snapshot.inner.rsi = siginfo_ptr;
                        thread_snapshot.inner.rdx = ucontext_ptr;

                        current_thread
                            .block_signals_for_handler(action.sig_handler_ignored_sigs, signal);
                        current_thread.enter_signal_handler(thread_snapshot);

                        ret = true;
                    }),
                }
            }
        }

        ret
    }

    fn default_signal_action(&mut self, signal: Signal) -> bool {
        match signal {
            Signal::Terminate
            | Signal::Kill
            | Signal::Interrupt
            | Signal::Quit
            | Signal::Abort
            | Signal::BusError
            | Signal::InvalidMemoryAccess
            | Signal::BrokenPipe
            | Signal::Hangup
            | Signal::FloatingPointError
            | Signal::IllegalInstruction
            | Signal::Trap
            | Signal::User1
            | Signal::User2
            | Signal::CpuTimeLimitExceeded
            | Signal::FileSizeLimitExceeded
            | Signal::BadSystemCall => {
                s_println!(
                    "fatal signal: pid={} signal={:?}",
                    self.pid.0,
                    signal
                );
                let threads = self.terminate_inner(signal as u64);
                let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
                for thread in threads {
                    thread_manager.mark_thread_exited(thread);
                }

                true
            }
            Signal::ChildChanged => false,
            Signal::Stop
            | Signal::TerminalStop
            | Signal::TerminalInput
            | Signal::TerminalOutput => {
                for process in self.group_id.get_processes() {
                    let threads = process.lock().threads.clone();
                    for weak in threads {
                        if let Some(thread) = weak.upgrade() {
                            thread.lock().state = State::Blocked(BlockType::Stopped);
                        }
                    }
                }
                true
            }
            Signal::Continue => unreachable!(),
            Signal::Alarm => false,
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
