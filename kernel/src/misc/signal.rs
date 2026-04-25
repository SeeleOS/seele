use crate::{
    misc::snapshot::Snapshot,
    object::linux_anon::wake_signalfd_for_process,
    process::{Process, ProcessRef, group::ProcessGroupID},
    s_println,
    thread::{
        THREAD_MANAGER, ThreadRef, get_current_thread,
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
    StackFault = 16,
    ChildChanged = 17,
    Continue = 18,
    Stop = 19,
    TerminalStop = 20,
    TerminalInput = 21,
    TerminalOutput = 22,
    UrgentCondition = 23,
    CpuTimeLimitExceeded = 24,
    FileSizeLimitExceeded = 25,
    VirtualAlarm = 26,
    ProfilingTimer = 27,
    WindowChanged = 28,
    IoPossible = 29,
    PowerFailure = 30,
    BadSystemCall = 31,
    Realtime0 = 34,
    Realtime1 = 35,
    Realtime2 = 36,
    Realtime3 = 37,
    Realtime4 = 38,
    Realtime5 = 39,
    Realtime6 = 40,
    Realtime7 = 41,
    Realtime8 = 42,
    Realtime9 = 43,
    Realtime10 = 44,
    Realtime11 = 45,
    Realtime12 = 46,
    Realtime13 = 47,
    Realtime14 = 48,
    Realtime15 = 49,
    Realtime16 = 50,
    Realtime17 = 51,
    Realtime18 = 52,
    Realtime19 = 53,
    Realtime20 = 54,
    Realtime21 = 55,
    Realtime22 = 56,
    Realtime23 = 57,
    Realtime24 = 58,
    Realtime25 = 59,
    Realtime26 = 60,
    Realtime27 = 61,
    Realtime28 = 62,
    Realtime29 = 63,
    Realtime30 = 64,
}

pub const SIGNAL_AMOUNT: usize = 64;

pub type SignalHandlerFn = extern "C" fn(i32);
pub type SigHandlerFn2 = extern "C" fn(i32, *const SigInfo, *const UContext);

#[repr(C)]
#[derive(Clone, Copy, Default)]
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
        self as usize - 1
    }

    pub const fn mask(self) -> u64 {
        1 << (self as u64 - 1)
    }

    pub const fn is_realtime(self) -> bool {
        (self as u64) >= Self::Realtime0 as u64
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
        const STACK_FAULT = Signal::StackFault.mask();
        const CHILD_CHANGED = Signal::ChildChanged.mask();
        const CONTINUE = Signal::Continue.mask();
        const STOP = Signal::Stop.mask();
        const TERMINAL_STOP = Signal::TerminalStop.mask();
        const TERMINAL_INPUT = Signal::TerminalInput.mask();
        const TERMINAL_OUTPUT = Signal::TerminalOutput.mask();
        const URGENT_CONDITION = Signal::UrgentCondition.mask();
        const CPU_TIME_LIMIT_EXCEEDED = Signal::CpuTimeLimitExceeded.mask();
        const FILE_SIZE_LIMIT_EXCEEDED = Signal::FileSizeLimitExceeded.mask();
        const VIRTUAL_ALARM = Signal::VirtualAlarm.mask();
        const PROFILING_TIMER = Signal::ProfilingTimer.mask();
        const WINDOW_CHANGED = Signal::WindowChanged.mask();
        const IO_POSSIBLE = Signal::IoPossible.mask();
        const POWER_FAILURE = Signal::PowerFailure.mask();
        const BAD_SYSTEM_CALL = Signal::BadSystemCall.mask();
    }
}

impl From<Signal> for Signals {
    fn from(value: Signal) -> Self {
        Self::from_bits_retain(value.mask())
    }
}

pub fn default_signal_action_vec() -> Vec<action::SignalAction> {
    alloc::vec![action::SignalAction::default(); SIGNAL_AMOUNT]
}

pub mod misc {
    pub use super::default_signal_action_vec;
}

#[derive(Default)]
pub struct ProcessSignalsResult {
    pub should_switch: bool,
    exited_threads: Vec<ThreadRef>,
    stopped_group: Option<ProcessGroupID>,
}

fn wake_process_threads(process: &ProcessRef, wake_stopped_only: bool) {
    let threads = {
        let process = process.lock();
        process.threads.clone()
    };

    let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
    for weak in threads {
        let Some(thread) = weak.upgrade() else {
            continue;
        };

        let should_wake = {
            let thread = thread.lock();
            match &thread.state {
                State::Blocked(BlockType::Stopped) => wake_stopped_only,
                State::Blocked(_) => !wake_stopped_only,
                _ => false,
            }
        };

        if should_wake {
            thread_manager.wake(thread);
        }
    }
}

pub fn send_signal_to_process(process: &ProcessRef, signal: Signal) {
    match signal {
        Signal::Continue => wake_process_threads(process, true),
        _ => {
            let pid = {
                let mut process = process.lock();
                process.pending_signals.insert(Signals::from(signal));
                process.pid.0
            };
            wake_signalfd_for_process(pid);
            wake_process_threads(process, false);
        }
    }
}

pub fn process_current_process_signals(process: &ProcessRef) -> bool {
    let result = {
        let mut process = process.lock();
        process.process_signals()
    };

    if let Some(group) = result.stopped_group {
        for process in group.get_processes() {
            let threads = {
                let process = process.lock();
                process.threads.clone()
            };

            for weak in threads {
                if let Some(thread) = weak.upgrade() {
                    thread.lock().state = State::Blocked(BlockType::Stopped);
                }
            }
        }
    }

    if !result.exited_threads.is_empty() {
        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
        for thread in result.exited_threads {
            thread_manager.mark_thread_exited(thread);
        }
    }

    result.should_switch
}

impl Process {
    pub fn get_signal_action(&mut self, signal: Signal) -> &mut action::SignalAction {
        &mut self.signal_actions[signal.index()]
    }

    /// Returns `true` if a user-space signal handler was installed and the
    /// caller should stop the current return path so the handler can run next.
    #[must_use]
    pub fn process_signals(&mut self) -> ProcessSignalsResult {
        let mut result = ProcessSignalsResult::default();

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
                        let default_result = self.default_signal_action(signal);
                        result.should_switch |= default_result.should_switch;
                        if !default_result.exited_threads.is_empty() {
                            result.exited_threads = default_result.exited_threads;
                        }
                        if default_result.stopped_group.is_some() {
                            result.stopped_group = default_result.stopped_group;
                        }
                    }
                    SignalHandlingType::Ignore => {}
                    SignalHandlingType::Function1(func) => with_current_thread(|current_thread| {
                        let (_, mut stack_builder) = self.addrspace.allocate_user_stack(16);
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

                        result.should_switch = true;
                    }),
                    SignalHandlingType::Function2(func) => with_current_thread(|current_thread| {
                        let (_, mut stack_builder) = self.addrspace.allocate_user_stack(16);
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

                        result.should_switch = true;
                    }),
                }
            }
        }

        result
    }

    fn default_signal_action(&mut self, signal: Signal) -> ProcessSignalsResult {
        if signal.is_realtime()
            || matches!(
                signal,
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
                    | Signal::StackFault
                    | Signal::Trap
                    | Signal::User1
                    | Signal::User2
                    | Signal::CpuTimeLimitExceeded
                    | Signal::FileSizeLimitExceeded
                    | Signal::VirtualAlarm
                    | Signal::ProfilingTimer
                    | Signal::IoPossible
                    | Signal::PowerFailure
                    | Signal::BadSystemCall
            )
        {
            s_println!("fatal signal: pid={} signal={:?}", self.pid.0, signal);
            let threads = self.terminate_inner(signal as u64);
            return ProcessSignalsResult {
                should_switch: true,
                exited_threads: threads,
                stopped_group: None,
            };
        }

        match signal {
            Signal::ChildChanged | Signal::UrgentCondition | Signal::WindowChanged => {
                ProcessSignalsResult::default()
            }
            Signal::Stop
            | Signal::TerminalStop
            | Signal::TerminalInput
            | Signal::TerminalOutput => ProcessSignalsResult {
                should_switch: true,
                exited_threads: Vec::new(),
                stopped_group: Some(self.group_id),
            },
            Signal::Continue => unreachable!(),
            Signal::Alarm => ProcessSignalsResult::default(),
            _ => ProcessSignalsResult::default(),
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
