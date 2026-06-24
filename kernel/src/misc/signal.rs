use crate::{
    misc::snapshot::Snapshot,
    object::linux_anon::wake_signalfd_for_process,
    process::{Process, ProcessExitStatus, ProcessRef, ptrace::report_signal_stop},
    thread::{
        ThreadRef,
        extended_state::update_active_user_extended_state_ptr_for_thread,
        get_current_thread,
        misc::{SnapshotState, State, with_current_thread},
        scheduling::request_all_cpus_resched,
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

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, TryFromPrimitive, Debug, EnumIter, PartialEq, Eq)]
#[repr(u64)]
pub enum Signal {
    SIGHUP = 1,
    SIGINT = 2,
    SIGQUIT = 3,
    SIGILL = 4,
    SIGTRAP = 5,
    SIGABRT = 6,
    SIGBUS = 7,
    SIGFPE = 8,
    SIGKILL = 9,
    SIGUSR1 = 10,
    SIGSEGV = 11,
    SIGUSR2 = 12,
    SIGPIPE = 13,
    SIGALRM = 14,
    SIGTERM = 15,
    SIGSTKFLT = 16,
    SIGCHLD = 17,
    SIGCONT = 18,
    SIGSTOP = 19,
    SIGTSTP = 20,
    SIGTTIN = 21,
    SIGTTOU = 22,
    SIGURG = 23,
    SIGXCPU = 24,
    SIGXFSZ = 25,
    SIGVTALRM = 26,
    SIGPROF = 27,
    SIGWINCH = 28,
    SIGIO = 29,
    SIGPWR = 30,
    SIGSYS = 31,
    SIGCANCEL = 32,
    SIGSETXID = 33,
    SIGRTMIN = 34,
    SIGRTMIN_PLUS_1 = 35,
    SIGRTMIN_PLUS_2 = 36,
    SIGRTMIN_PLUS_3 = 37,
    SIGRTMIN_PLUS_4 = 38,
    SIGRTMIN_PLUS_5 = 39,
    SIGRTMIN_PLUS_6 = 40,
    SIGRTMIN_PLUS_7 = 41,
    SIGRTMIN_PLUS_8 = 42,
    SIGRTMIN_PLUS_9 = 43,
    SIGRTMIN_PLUS_10 = 44,
    SIGRTMIN_PLUS_11 = 45,
    SIGRTMIN_PLUS_12 = 46,
    SIGRTMIN_PLUS_13 = 47,
    SIGRTMIN_PLUS_14 = 48,
    SIGRTMIN_PLUS_15 = 49,
    SIGRTMIN_PLUS_16 = 50,
    SIGRTMIN_PLUS_17 = 51,
    SIGRTMIN_PLUS_18 = 52,
    SIGRTMIN_PLUS_19 = 53,
    SIGRTMIN_PLUS_20 = 54,
    SIGRTMIN_PLUS_21 = 55,
    SIGRTMIN_PLUS_22 = 56,
    SIGRTMIN_PLUS_23 = 57,
    SIGRTMIN_PLUS_24 = 58,
    SIGRTMIN_PLUS_25 = 59,
    SIGRTMIN_PLUS_26 = 60,
    SIGRTMIN_PLUS_27 = 61,
    SIGRTMIN_PLUS_28 = 62,
    SIGRTMIN_PLUS_29 = 63,
    SIGRTMIN_PLUS_30 = 64,
}

pub const SIGNAL_AMOUNT: usize = 64;
pub const SI_USER: i32 = 0;
pub const SI_QUEUE: i32 = -1;
pub const SI_TKILL: i32 = -6;

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

#[derive(Clone, Copy, Debug, Default)]
pub struct PendingSignalInfo {
    pub si_signo: i32,
    pub si_errno: i32,
    pub si_code: i32,
    pub si_pid: i32,
    pub si_uid: u32,
    pub si_value: u64,
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

    pub fn for_process_signal(signal: Signal, sender_pid: i32, sender_uid: u32) -> Self {
        Self {
            si_signo: signal as i32,
            si_code: SI_USER,
            fields: SigInfoFields {
                value: SigInfoValue {
                    si_pid: sender_pid,
                    si_uid: sender_uid,
                    ..Default::default()
                },
            },
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

    pub fn sender_pid(&self) -> i32 {
        unsafe { self.fields.value.si_pid }
    }

    pub fn sender_uid(&self) -> u32 {
        unsafe { self.fields.value.si_uid }
    }

    pub fn signal_value_int(&self) -> i32 {
        unsafe { self.fields.value.si_value.sival_int }
    }

    pub fn signal_value_ptr(&self) -> u64 {
        unsafe { self.fields.value.si_value.sival_ptr as usize as u64 }
    }
}

impl PendingSignalInfo {
    pub fn for_signal(signal: Signal) -> Self {
        Self {
            si_signo: signal as i32,
            ..Default::default()
        }
    }

    pub fn from_siginfo(siginfo: SigInfo) -> Self {
        Self {
            si_signo: siginfo.si_signo,
            si_errno: siginfo.si_errno,
            si_code: siginfo.si_code,
            si_pid: siginfo.sender_pid(),
            si_uid: siginfo.sender_uid(),
            si_value: siginfo.signal_value_ptr(),
        }
    }

    pub fn to_siginfo(self) -> SigInfo {
        SigInfo {
            si_signo: self.si_signo,
            si_errno: self.si_errno,
            si_code: self.si_code,
            fields: SigInfoFields {
                value: SigInfoValue {
                    si_pid: self.si_pid,
                    si_uid: self.si_uid,
                    si_value: SigValue {
                        sival_ptr: self.si_value as usize as *mut c_void,
                    },
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
        (self as u64) >= Self::SIGRTMIN as u64
    }

    pub const fn is_unblockable(self) -> bool {
        matches!(self, Self::SIGKILL | Self::SIGSTOP)
    }
}

bitflags! {
    #[derive(Default, Clone, Copy, Debug)]
    #[repr(transparent)]
    pub struct Signals: u64 {
        const SIGHUP = Signal::SIGHUP.mask();
        const SIGINT = Signal::SIGINT.mask();
        const SIGQUIT = Signal::SIGQUIT.mask();
        const SIGILL = Signal::SIGILL.mask();
        const SIGTRAP = Signal::SIGTRAP.mask();
        const SIGABRT = Signal::SIGABRT.mask();
        const SIGBUS = Signal::SIGBUS.mask();
        const SIGFPE = Signal::SIGFPE.mask();
        const SIGKILL = Signal::SIGKILL.mask();
        const SIGUSR1 = Signal::SIGUSR1.mask();
        const SIGSEGV = Signal::SIGSEGV.mask();
        const SIGUSR2 = Signal::SIGUSR2.mask();
        const SIGPIPE = Signal::SIGPIPE.mask();
        const SIGALRM = Signal::SIGALRM.mask();
        const SIGTERM = Signal::SIGTERM.mask();
        const SIGSTKFLT = Signal::SIGSTKFLT.mask();
        const SIGCHLD = Signal::SIGCHLD.mask();
        const SIGCONT = Signal::SIGCONT.mask();
        const SIGSTOP = Signal::SIGSTOP.mask();
        const SIGTSTP = Signal::SIGTSTP.mask();
        const SIGTTIN = Signal::SIGTTIN.mask();
        const SIGTTOU = Signal::SIGTTOU.mask();
        const SIGURG = Signal::SIGURG.mask();
        const SIGXCPU = Signal::SIGXCPU.mask();
        const SIGXFSZ = Signal::SIGXFSZ.mask();
        const SIGVTALRM = Signal::SIGVTALRM.mask();
        const SIGPROF = Signal::SIGPROF.mask();
        const SIGWINCH = Signal::SIGWINCH.mask();
        const SIGIO = Signal::SIGIO.mask();
        const SIGPWR = Signal::SIGPWR.mask();
        const SIGSYS = Signal::SIGSYS.mask();
        const SIGCANCEL = Signal::SIGCANCEL.mask();
        const SIGSETXID = Signal::SIGSETXID.mask();
        const SIGRTMIN = Signal::SIGRTMIN.mask();
        const SIGRTMIN_PLUS_1 = Signal::SIGRTMIN_PLUS_1.mask();
        const SIGRTMIN_PLUS_2 = Signal::SIGRTMIN_PLUS_2.mask();
        const SIGRTMIN_PLUS_3 = Signal::SIGRTMIN_PLUS_3.mask();
        const SIGRTMIN_PLUS_4 = Signal::SIGRTMIN_PLUS_4.mask();
        const SIGRTMIN_PLUS_5 = Signal::SIGRTMIN_PLUS_5.mask();
        const SIGRTMIN_PLUS_6 = Signal::SIGRTMIN_PLUS_6.mask();
        const SIGRTMIN_PLUS_7 = Signal::SIGRTMIN_PLUS_7.mask();
        const SIGRTMIN_PLUS_8 = Signal::SIGRTMIN_PLUS_8.mask();
        const SIGRTMIN_PLUS_9 = Signal::SIGRTMIN_PLUS_9.mask();
        const SIGRTMIN_PLUS_10 = Signal::SIGRTMIN_PLUS_10.mask();
        const SIGRTMIN_PLUS_11 = Signal::SIGRTMIN_PLUS_11.mask();
        const SIGRTMIN_PLUS_12 = Signal::SIGRTMIN_PLUS_12.mask();
        const SIGRTMIN_PLUS_13 = Signal::SIGRTMIN_PLUS_13.mask();
        const SIGRTMIN_PLUS_14 = Signal::SIGRTMIN_PLUS_14.mask();
        const SIGRTMIN_PLUS_15 = Signal::SIGRTMIN_PLUS_15.mask();
        const SIGRTMIN_PLUS_16 = Signal::SIGRTMIN_PLUS_16.mask();
        const SIGRTMIN_PLUS_17 = Signal::SIGRTMIN_PLUS_17.mask();
        const SIGRTMIN_PLUS_18 = Signal::SIGRTMIN_PLUS_18.mask();
        const SIGRTMIN_PLUS_19 = Signal::SIGRTMIN_PLUS_19.mask();
        const SIGRTMIN_PLUS_20 = Signal::SIGRTMIN_PLUS_20.mask();
        const SIGRTMIN_PLUS_21 = Signal::SIGRTMIN_PLUS_21.mask();
        const SIGRTMIN_PLUS_22 = Signal::SIGRTMIN_PLUS_22.mask();
        const SIGRTMIN_PLUS_23 = Signal::SIGRTMIN_PLUS_23.mask();
        const SIGRTMIN_PLUS_24 = Signal::SIGRTMIN_PLUS_24.mask();
        const SIGRTMIN_PLUS_25 = Signal::SIGRTMIN_PLUS_25.mask();
        const SIGRTMIN_PLUS_26 = Signal::SIGRTMIN_PLUS_26.mask();
        const SIGRTMIN_PLUS_27 = Signal::SIGRTMIN_PLUS_27.mask();
        const SIGRTMIN_PLUS_28 = Signal::SIGRTMIN_PLUS_28.mask();
        const SIGRTMIN_PLUS_29 = Signal::SIGRTMIN_PLUS_29.mask();
        const SIGRTMIN_PLUS_30 = Signal::SIGRTMIN_PLUS_30.mask();
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
    stop_current: bool,
    stopped_signal: Option<Signal>,
}

impl ProcessSignalsResult {
    fn merge(&mut self, other: Self) {
        self.should_switch |= other.should_switch;
        self.stop_current |= other.stop_current;
        if self.stopped_signal.is_none() {
            self.stopped_signal = other.stopped_signal;
        }
        self.exited_threads.extend(other.exited_threads);
    }
}

fn wake_process_threads(process: &ProcessRef, wake_stopped_only: bool) {
    let threads = {
        let process = process.lock();
        process.threads.clone()
    };

    crate::thread::with_thread_manager(|thread_manager| {
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
    });
}

fn wake_specific_thread_for_signal(thread: &ThreadRef, signal: Signal) {
    let should_wake = {
        let mut thread = thread.lock();
        let should_wake = matches!(
            &thread.state,
            State::Blocking(block_type) | State::Blocked(block_type)
                if !matches!(block_type, BlockType::Stopped)
        );
        let interrupts_wait = !thread_blocks_signal_inner(&thread, signal);
        if thread.interruptible_wait_active && interrupts_wait {
            thread.interrupted_by_signal = true;
        }
        if should_wake && interrupts_wait {
            thread.interrupted_by_signal = true;
            true
        } else {
            false
        }
    };

    if should_wake {
        crate::thread::with_thread_manager(|manager| manager.wake(thread.clone()));
    }
}

fn thread_blocks_signal_inner(thread: &Thread, signal: Signal) -> bool {
    if signal.is_unblockable() {
        return false;
    }
    thread.blocked_signals.contains(Signals::from(signal))
}

fn wake_process_threads_for_signal(process: &ProcessRef, signal: Signal) {
    let threads = {
        let process = process.lock();
        process.threads.clone()
    };

    for weak in threads {
        let Some(thread) = weak.upgrade() else {
            continue;
        };
        wake_specific_thread_for_signal(&thread, signal);
    }
}

fn queue_signal(process: &ProcessRef, signal: Signal, siginfo: Option<SigInfo>) {
    match signal {
        Signal::SIGCONT => wake_process_threads(process, true),
        _ => {
            let pid = {
                let mut process = process.lock();
                let signal_bits = Signals::from(signal);
                let already_pending = process.pending_signals.contains(signal_bits);
                process.pending_signals.insert(signal_bits);
                if let Some(siginfo) = siginfo
                    && (!already_pending || process.pending_signal_info[signal.index()].is_none())
                {
                    process.pending_signal_info[signal.index()] =
                        Some(PendingSignalInfo::from_siginfo(siginfo));
                }
                process.pid.0
            };
            wake_signalfd_for_process(pid);
            request_all_cpus_resched();
            wake_process_threads_for_signal(process, signal);
        }
    }
}

pub fn send_signal_to_process(process: &ProcessRef, signal: Signal) {
    queue_signal(process, signal, None);
}

pub fn send_signal_to_process_with_siginfo(process: &ProcessRef, signal: Signal, siginfo: SigInfo) {
    queue_signal(process, signal, Some(siginfo));
}

fn queue_signal_to_thread(thread: &ThreadRef, signal: Signal, siginfo: Option<SigInfo>) {
    let parent = {
        let mut thread = thread.lock();
        let signal_bits = Signals::from(signal);
        let already_pending = thread.pending_signals.contains(signal_bits);
        thread.pending_signals.insert(signal_bits);
        if let Some(siginfo) = siginfo
            && (!already_pending || thread.pending_signal_info[signal.index()].is_none())
        {
            thread.pending_signal_info[signal.index()] =
                Some(PendingSignalInfo::from_siginfo(siginfo));
        }
        thread.parent.clone()
    };

    let pid = parent.lock().pid.0;
    wake_signalfd_for_process(pid);
    request_all_cpus_resched();
    wake_specific_thread_for_signal(thread, signal);
}

pub fn send_signal_to_thread(thread: &ThreadRef, signal: Signal) {
    queue_signal_to_thread(thread, signal, None);
}

pub fn send_signal_to_thread_with_siginfo(thread: &ThreadRef, signal: Signal, siginfo: SigInfo) {
    queue_signal_to_thread(thread, signal, Some(siginfo));
}

pub fn process_current_process_signals(process: &ProcessRef) -> bool {
    let thread_ref = get_current_thread();
    {
        let thread = thread_ref.lock();
        if thread.temporary_blocked_signals.is_some() && thread.interruptible_wait_active {
            drop(thread);
            thread_ref.lock().interrupted_by_signal = true;
            return false;
        }
        if thread.interruptible_wait_active
            && process_has_pending_user_handler_signal(process, thread.blocked_signals)
        {
            drop(thread);
            thread_ref.lock().interrupted_by_signal = true;
            return false;
        }
    }

    let (blocked_signals, mut pending_signals, mut pending_signal_info) = {
        let mut thread = thread_ref.lock();
        (
            thread.blocked_signals,
            mem::take(&mut thread.pending_signals),
            mem::take(&mut thread.pending_signal_info),
        )
    };
    let result = {
        let mut process = process.lock();
        let mut result = process_pending_signals(
            &mut process,
            &mut pending_signals,
            &mut pending_signal_info,
            blocked_signals,
        );
        result.merge(process.process_signals(blocked_signals));
        result
    };
    {
        let mut thread = thread_ref.lock();
        if result.should_switch && thread.interruptible_wait_active {
            thread.interrupted_by_signal = true;
        }
        thread.pending_signals = pending_signals;
        thread.pending_signal_info = pending_signal_info;
    }

    if let Some(signal) = result.stopped_signal {
        report_signal_stop(process, signal);
    }

    if result.stop_current {
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

    if !result.exited_threads.is_empty() {
        crate::thread::with_thread_manager(|thread_manager| {
            for thread in result.exited_threads {
                thread_manager.mark_thread_exited(thread);
            }
        });
    }

    result.should_switch
}

fn process_has_pending_user_handler_signal(process: &ProcessRef, blocked_signals: Signals) -> bool {
    let process = process.lock();
    Signal::iter().any(|signal| {
        let signal_bits = Signals::from(signal);
        if !process.pending_signals.contains(signal_bits) {
            return false;
        }
        if !signal.is_unblockable() && blocked_signals.contains(signal_bits) {
            return false;
        }

        let action = &process.signal_actions[signal.index()];
        matches!(
            action.handling_type,
            SignalHandlingType::Function1(_) | SignalHandlingType::Function2(_)
        )
    })
}

impl Process {
    pub fn get_signal_action(&mut self, signal: Signal) -> &mut action::SignalAction {
        &mut self.signal_actions[signal.index()]
    }

    /// Returns `true` if a user-space signal handler was installed and the
    /// caller should stop the current return path so the handler can run next.
    #[must_use]
    pub fn process_signals(&mut self, blocked_signals: Signals) -> ProcessSignalsResult {
        let mut pending_signals = mem::take(&mut self.pending_signals);
        let mut pending_signal_info = mem::take(&mut self.pending_signal_info);
        let result = process_pending_signals(
            self,
            &mut pending_signals,
            &mut pending_signal_info,
            blocked_signals,
        );
        self.pending_signals = pending_signals;
        self.pending_signal_info = pending_signal_info;
        result
    }

    fn default_signal_action(&mut self, signal: Signal) -> ProcessSignalsResult {
        if signal.is_realtime()
            || matches!(
                signal,
                Signal::SIGTERM
                    | Signal::SIGKILL
                    | Signal::SIGINT
                    | Signal::SIGQUIT
                    | Signal::SIGABRT
                    | Signal::SIGBUS
                    | Signal::SIGSEGV
                    | Signal::SIGPIPE
                    | Signal::SIGALRM
                    | Signal::SIGHUP
                    | Signal::SIGFPE
                    | Signal::SIGILL
                    | Signal::SIGSTKFLT
                    | Signal::SIGTRAP
                    | Signal::SIGUSR1
                    | Signal::SIGUSR2
                    | Signal::SIGXCPU
                    | Signal::SIGXFSZ
                    | Signal::SIGVTALRM
                    | Signal::SIGPROF
                    | Signal::SIGIO
                    | Signal::SIGPWR
                    | Signal::SIGSYS
            )
        {
            let threads = self.terminate_inner(ProcessExitStatus::Signaled(signal));
            return ProcessSignalsResult {
                should_switch: true,
                exited_threads: threads,
                stop_current: false,
                stopped_signal: None,
            };
        }

        match signal {
            Signal::SIGCHLD | Signal::SIGURG | Signal::SIGWINCH => ProcessSignalsResult::default(),
            Signal::SIGSTOP | Signal::SIGTSTP | Signal::SIGTTIN | Signal::SIGTTOU => {
                ProcessSignalsResult {
                    should_switch: true,
                    exited_threads: Vec::new(),
                    stop_current: true,
                    stopped_signal: Some(signal),
                }
            }
            Signal::SIGCONT => unreachable!(),
            _ => ProcessSignalsResult::default(),
        }
    }
}

fn process_pending_signals(
    process: &mut Process,
    pending_signals: &mut Signals,
    pending_signal_info: &mut [Option<PendingSignalInfo>],
    blocked_signals: Signals,
) -> ProcessSignalsResult {
    let mut result = ProcessSignalsResult::default();

    for signal in Signal::iter() {
        let signal_bits = Signals::from(signal);
        if pending_signals.contains(signal_bits)
            && (signal.is_unblockable() || !blocked_signals.contains(signal_bits))
        {
            let action = process.signal_actions[signal.index()].clone();
            pending_signals.remove(signal_bits);
            let siginfo = pending_signal_info[signal.index()]
                .take()
                .unwrap_or_else(|| PendingSignalInfo::for_signal(signal))
                .to_siginfo();

            match action.handling_type {
                SignalHandlingType::Default => {
                    result.merge(process.default_signal_action(signal));
                }
                SignalHandlingType::Ignore => {}
                SignalHandlingType::Function1(func) => with_current_thread(|current_thread| {
                    let saved_mask = current_thread.restore_temporary_blocked_signals();
                    let handler_stack = prepare_signal_handler_stack(
                        process,
                        current_thread,
                        action.flags,
                        action.restorer,
                    );

                    let (current_extended_state, current_fs_base) = {
                        let snapshot = current_thread.get_appropriate_snapshot();
                        (snapshot.extended_state.clone(), snapshot.fs_base)
                    };
                    let mut thread_snapshot = ThreadSnapshot::new_with_extended_state(
                        (func as usize) as u64,
                        &mut process.addrspace,
                        handler_stack,
                        current_thread.kernel_stack_top,
                        ThreadSnapshotType::Thread,
                        current_extended_state,
                    );
                    thread_snapshot.fs_base = current_fs_base;

                    thread_snapshot.inner.rdi = signal as u64;

                    current_thread.block_signals_for_handler(
                        action.sig_handler_ignored_sigs,
                        signal,
                        saved_mask,
                    );
                    current_thread.enter_signal_handler(thread_snapshot);

                    result.should_switch = true;
                }),
                SignalHandlingType::Function2(func) => with_current_thread(|current_thread| {
                    let (_, mut frame_builder) = process.addrspace.allocate_user(1);

                    let saved_mask = current_thread.restore_temporary_blocked_signals();
                    let ucontext = build_signal_ucontext(current_thread);

                    let ucontext_ptr = frame_builder.push_struct(&ucontext);
                    let siginfo_ptr = frame_builder.push_struct(&siginfo);

                    let handler_stack = prepare_signal_handler_stack(
                        process,
                        current_thread,
                        action.flags,
                        action.restorer,
                    );

                    let (current_extended_state, current_fs_base) = {
                        let snapshot = current_thread.get_appropriate_snapshot();
                        (snapshot.extended_state.clone(), snapshot.fs_base)
                    };
                    let mut thread_snapshot = ThreadSnapshot::new_with_extended_state(
                        (func as usize) as u64,
                        &mut process.addrspace,
                        handler_stack,
                        current_thread.kernel_stack_top,
                        ThreadSnapshotType::Thread,
                        current_extended_state,
                    );
                    thread_snapshot.fs_base = current_fs_base;

                    thread_snapshot.inner.rdi = signal as u64;
                    thread_snapshot.inner.rsi = siginfo_ptr;
                    thread_snapshot.inner.rdx = ucontext_ptr;

                    current_thread.block_signals_for_handler(
                        action.sig_handler_ignored_sigs,
                        signal,
                        saved_mask,
                    );
                    current_thread.enter_signal_handler(thread_snapshot);

                    result.should_switch = true;
                }),
            }
        }
    }

    result
}

const SA_ONSTACK: u64 = 0x0800_0000;

fn prepare_signal_handler_stack(
    process: &mut Process,
    thread: &Thread,
    action_flags: u64,
    restorer: usize,
) -> u64 {
    if action_flags & SA_ONSTACK != 0
        && let Some(stack_top) = thread.enabled_sigaltstack_top()
    {
        // x86_64 SysV requires %rsp % 16 == 8 on function entry.
        let rsp = (stack_top - 16) & !0xf;
        let padding_ptr = rsp as *mut u64;
        let restorer_ptr = (rsp + 8) as *mut u64;
        let _ = process.addrspace.write(padding_ptr, &0);
        let _ = process.addrspace.write(restorer_ptr, &(restorer as u64));
        return restorer_ptr as u64;
    }

    let (_, mut stack_builder) = process.addrspace.allocate_user_stack(16);
    stack_builder.push(0);
    stack_builder.push(restorer as u64);
    stack_builder.finish().as_u64()
}

impl Thread {
    fn enabled_sigaltstack_top(&self) -> Option<u64> {
        const SS_DISABLE: i32 = 2;

        if self.sigaltstack.ss_flags & SS_DISABLE != 0
            || self.sigaltstack.ss_sp == 0
            || self.sigaltstack.ss_size == 0
        {
            return None;
        }
        Some(
            self.sigaltstack
                .ss_sp
                .saturating_add(self.sigaltstack.ss_size as u64),
        )
    }

    fn restore_temporary_blocked_signals(&mut self) -> Signals {
        if let Some((old_mask, _)) = self.temporary_blocked_signals.take() {
            self.blocked_signals = old_mask;
            old_mask
        } else {
            self.blocked_signals
        }
    }

    fn block_signals_for_handler(
        &mut self,
        mut signals_to_block: Signals,
        signal: Signal,
        saved_mask: Signals,
    ) {
        signals_to_block.insert(Signals::from(signal));
        self.saved_blocked_signals.push(saved_mask);
        self.blocked_signals.insert(signals_to_block);
    }

    fn enter_signal_handler(&mut self, snapshot: ThreadSnapshot) {
        self.snapshot_state = SnapshotState::SignalHandler;
        self.sig_handler_snapshot = snapshot;
        update_active_user_extended_state_ptr_for_thread(self);
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
