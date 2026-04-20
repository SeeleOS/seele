use crate::{
    misc::snapshot::Snapshot,
    object::linux_anon::wake_signalfd_for_process,
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
use bitflags::bitflags;
use core::{ffi::c_void, mem};
use num_enum::TryFromPrimitive;
use strum::{EnumIter, IntoEnumIterator};

pub mod action {
    pub use super::{SignalAction, SignalHandlingType, Signals};
}

#[derive(Clone, Copy, TryFromPrimitive, Debug, EnumIter)]
#[repr(u64)]
pub enum Signal {
    Hangup = 1,
    Interrupt = 2,
    Quit = 3,
    IllegalInstruction = 4,
    Trap = 5,
    Abort = 6,
    BusError = 7,
    FloatingPointError = 8,
    Kill = 9,
    User1 = 10,
    InvalidMemoryAccess = 11,
    User2 = 12,
    BrokenPipe = 13,
    Alarm = 14,
    Terminate = 15,
    ChildChanged = 17,
    Continue = 18,
    Stop = 19,
    TerminalStop = 20,
    TerminalInput = 21,
    TerminalOutput = 22,
    CpuTimeLimitExceeded = 24,
    FileSizeLimitExceeded = 25,
    BadSystemCall = 31,
}

pub const SIGNAL_AMOUNT: usize = 24;

pub type SignalHandlerFn = extern "C" fn(i32);
pub type SigHandlerFn2 = extern "C" fn(i32, *const SigInfo, *const UContext);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SigInfo {
    pub si_signo: i32,
    pub si_errno: i32,
    pub si_code: i32,
    _pad0: i32,
    fields: SigInfoFields,
}

#[repr(C)]
#[derive(Clone, Copy)]
union SigInfoFields {
    pad: [u8; 112],
    child: SigInfoChild,
    fault: SigInfoFault,
    value: SigInfoValue,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SigInfoChild {
    si_pid: i32,
    si_uid: u32,
    si_status: i32,
    _pad1: i32,
    si_utime: i64,
    si_stime: i64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SigInfoFault {
    si_addr: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SigInfoValue {
    si_pid: i32,
    si_uid: u32,
    si_value: SigValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union SigValue {
    pub sival_int: i32,
    pub sival_ptr: *mut c_void,
}

impl Default for SigValue {
    fn default() -> Self {
        Self { sival_int: 0 }
    }
}

impl Default for SigInfoFields {
    fn default() -> Self {
        Self { pad: [0; 112] }
    }
}

impl Default for SigInfo {
    fn default() -> Self {
        Self {
            si_signo: 0,
            si_errno: 0,
            si_code: 0,
            _pad0: 0,
            fields: SigInfoFields::default(),
        }
    }
}

impl SigInfo {
    pub fn for_signal(signal: Signal) -> Self {
        Self {
            si_signo: signal as i32,
            ..Default::default()
        }
    }

    pub fn for_waitid(signal: Signal, code: i32, pid: i32, status: i32) -> Self {
        Self {
            si_signo: signal as i32,
            si_code: code,
            fields: SigInfoFields {
                child: SigInfoChild {
                    si_pid: pid,
                    si_status: status,
                    ..Default::default()
                },
            },
            ..Default::default()
        }
    }
}

const _: [(); 128] = [(); mem::size_of::<SigInfo>()];

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct UContext {
    pub blocked_signals: u64,
    pub gregs: [u64; 20],
}

#[derive(Default, Clone, Debug)]
#[repr(C)]
pub struct SignalAction {
    pub handling_type: SignalHandlingType,
    pub sig_handler_ignored_sigs: Signals,
    pub flags: u64,
    pub restorer: usize,
}

#[derive(Default, Clone, Debug)]
#[repr(C)]
pub enum SignalHandlingType {
    #[default]
    Default,
    Ignore,
    Function1(SignalHandlerFn),
    Function2(SigHandlerFn2),
}

impl Signal {
    pub const fn index(self) -> usize {
        match self {
            Self::Hangup => 0,
            Self::Interrupt => 1,
            Self::Quit => 2,
            Self::IllegalInstruction => 3,
            Self::Trap => 4,
            Self::Abort => 5,
            Self::BusError => 6,
            Self::FloatingPointError => 7,
            Self::Kill => 8,
            Self::User1 => 9,
            Self::InvalidMemoryAccess => 10,
            Self::User2 => 11,
            Self::BrokenPipe => 12,
            Self::Alarm => 13,
            Self::Terminate => 14,
            Self::ChildChanged => 15,
            Self::Continue => 16,
            Self::Stop => 17,
            Self::TerminalStop => 18,
            Self::TerminalInput => 19,
            Self::TerminalOutput => 20,
            Self::CpuTimeLimitExceeded => 21,
            Self::FileSizeLimitExceeded => 22,
            Self::BadSystemCall => 23,
        }
    }

    pub const fn mask(self) -> u64 {
        1 << (self as u64 - 1)
    }
}

bitflags! {
    #[derive(Default, Clone, Copy, Debug)]
    #[repr(transparent)]
    pub struct Signals: u64 {
        const HANGUP = Signal::Hangup.mask();
        const INTERRUPT = Signal::Interrupt.mask();
        const QUIT = Signal::Quit.mask();
        const ILLEGAL_INSTRUCTION = Signal::IllegalInstruction.mask();
        const TRAP = Signal::Trap.mask();
        const ABORT = Signal::Abort.mask();
        const BUS_ERROR = Signal::BusError.mask();
        const FLOATING_POINT_ERROR = Signal::FloatingPointError.mask();
        const KILL = Signal::Kill.mask();
        const USER1 = Signal::User1.mask();
        const INVALID_MEMORY_ACCESS = Signal::InvalidMemoryAccess.mask();
        const USER2 = Signal::User2.mask();
        const BROKEN_PIPE = Signal::BrokenPipe.mask();
        const ALARM = Signal::Alarm.mask();
        const TERMINATE = Signal::Terminate.mask();
        const CHILD_CHANGED = Signal::ChildChanged.mask();
        const CONTINUE = Signal::Continue.mask();
        const STOP = Signal::Stop.mask();
        const TERMINAL_STOP = Signal::TerminalStop.mask();
        const TERMINAL_INPUT = Signal::TerminalInput.mask();
        const TERMINAL_OUTPUT = Signal::TerminalOutput.mask();
        const CPU_TIME_LIMIT_EXCEEDED = Signal::CpuTimeLimitExceeded.mask();
        const FILE_SIZE_LIMIT_EXCEEDED = Signal::FileSizeLimitExceeded.mask();
        const BAD_SYSTEM_CALL = Signal::BadSystemCall.mask();
    }
}

impl From<Signal> for Signals {
    fn from(value: Signal) -> Self {
        match value {
            Signal::Hangup => Self::HANGUP,
            Signal::Interrupt => Self::INTERRUPT,
            Signal::Quit => Self::QUIT,
            Signal::IllegalInstruction => Self::ILLEGAL_INSTRUCTION,
            Signal::Trap => Self::TRAP,
            Signal::Abort => Self::ABORT,
            Signal::BusError => Self::BUS_ERROR,
            Signal::FloatingPointError => Self::FLOATING_POINT_ERROR,
            Signal::Kill => Self::KILL,
            Signal::User1 => Self::USER1,
            Signal::InvalidMemoryAccess => Self::INVALID_MEMORY_ACCESS,
            Signal::User2 => Self::USER2,
            Signal::BrokenPipe => Self::BROKEN_PIPE,
            Signal::Alarm => Self::ALARM,
            Signal::Terminate => Self::TERMINATE,
            Signal::ChildChanged => Self::CHILD_CHANGED,
            Signal::Continue => Self::CONTINUE,
            Signal::Stop => Self::STOP,
            Signal::TerminalStop => Self::TERMINAL_STOP,
            Signal::TerminalInput => Self::TERMINAL_INPUT,
            Signal::TerminalOutput => Self::TERMINAL_OUTPUT,
            Signal::CpuTimeLimitExceeded => Self::CPU_TIME_LIMIT_EXCEEDED,
            Signal::FileSizeLimitExceeded => Self::FILE_SIZE_LIMIT_EXCEEDED,
            Signal::BadSystemCall => Self::BAD_SYSTEM_CALL,
        }
    }
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
        if matches!(signal, Signal::Kill) {
            s_println!(
                "send_signal trace target_pid={} signal={:?}",
                self.pid.0,
                signal
            );
        }
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
                wake_signalfd_for_process(self.pid.0);
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

                        let current_fx_state = current_thread.get_appropriate_snapshot().fx_state;
                        let mut thread_snapshot = ThreadSnapshot::new_with_fx_state(
                            (func as usize) as u64,
                            &mut self.addrspace,
                            stack_builder.finish().as_u64(),
                            ThreadSnapshotType::Thread,
                            current_fx_state,
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

                        let siginfo = SigInfo::for_signal(signal);
                        let ucontext = build_signal_ucontext(current_thread);

                        let ucontext_ptr = frame_builder.push_struct(&ucontext);
                        let siginfo_ptr = frame_builder.push_struct(&siginfo);

                        // Keep the signal handler entry stack ABI-aligned.
                        stack_builder.push(0);
                        stack_builder.push(action.restorer as u64);

                        let current_fx_state = current_thread.get_appropriate_snapshot().fx_state;
                        let mut thread_snapshot = ThreadSnapshot::new_with_fx_state(
                            (func as usize) as u64,
                            &mut self.addrspace,
                            stack_builder.finish().as_u64(),
                            ThreadSnapshotType::Thread,
                            current_fx_state,
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
                s_println!("fatal signal: pid={} signal={:?}", self.pid.0, signal);
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
