use crate::process::group::ProcessGroupID;
use crate::process::manager::MANAGER;
use crate::process::misc::ProcessID;
use crate::signal::action::{SignalHandlingType, Signals};
use crate::systemcall::utils::*;
use crate::thread::misc::SnapshotState;
use crate::thread::scheduling::return_to_executor_no_save;
use crate::thread::{THREAD_MANAGER, get_current_thread};
use crate::{
    define_syscall,
    memory::user_safe,
    process::manager::get_current_process,
    signal::{Signal, action::SignalAction},
};
use core::mem::size_of;
use num_enum::TryFromPrimitive;
use spin::Mutex;

const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;
const SS_ONSTACK: i32 = 1;
const SS_DISABLE: i32 = 2;
const MINSIGSTKSZ: usize = 2048;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxStack {
    ss_sp: u64,
    ss_flags: i32,
    ss_size: usize,
}

static SIGALTSTACK_STATE: Mutex<LinuxStack> = Mutex::new(LinuxStack {
    ss_sp: 0,
    ss_flags: SS_DISABLE,
    ss_size: 0,
});

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct SigActionFlags: u64 {
        const SIGINFO = 0x0000_0004;
    }
}

#[derive(Clone, Copy, Debug, TryFromPrimitive)]
#[repr(i32)]
enum SigMaskHow {
    Block = 0,
    Unblock = 1,
    SetMask = 2,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxSigAction {
    handler: usize,
    flags: u64,
    restorer: usize,
    mask: u64,
}

fn encode_sigaction(action: &SignalAction) -> LinuxSigAction {
    let (handler, extra_flags) = match action.handling_type {
        SignalHandlingType::Default => (SIG_DFL, 0),
        SignalHandlingType::Ignore => (SIG_IGN, 0),
        SignalHandlingType::Function1(func) => (func as usize, 0),
        SignalHandlingType::Function2(func) => (func as usize, SigActionFlags::SIGINFO.bits()),
    };

    LinuxSigAction {
        handler,
        flags: action.flags | extra_flags,
        restorer: action.restorer,
        mask: action.sig_handler_ignored_sigs.bits(),
    }
}

fn decode_sigaction(action: LinuxSigAction) -> SignalAction {
    let handling_type = match action.handler {
        SIG_DFL => SignalHandlingType::Default,
        SIG_IGN => SignalHandlingType::Ignore,
        handler
            if SigActionFlags::from_bits_truncate(action.flags)
                .contains(SigActionFlags::SIGINFO) =>
        unsafe { SignalHandlingType::Function2(core::mem::transmute(handler)) },
        handler => unsafe { SignalHandlingType::Function1(core::mem::transmute(handler)) },
    };

    SignalAction {
        handling_type,
        sig_handler_ignored_sigs: Signals::from_bits_truncate(action.mask),
        flags: action.flags,
        restorer: action.restorer,
    }
}

define_syscall!(
    RtSigaction,
    |signal: i32, new_action: u64, old_action: u64, sigsetsize: usize| {
        if sigsetsize != size_of::<u64>() {
            return Err(SyscallError::InvalidArguments);
        }

        let signal = Signal::try_from(signal as u64).map_err(|_| SyscallError::InvalidArguments)?;
        let new_action = new_action as *const LinuxSigAction;
        let old_action = old_action as *mut LinuxSigAction;
        let new_action_decoded = unsafe { (!new_action.is_null()).then(|| decode_sigaction(*new_action)) };
        let (pid, old_encoded) = {
            let process = get_current_process();
            let mut process = process.lock();
            let pid = process.pid.0;
            let current_signal_action = process.get_signal_action(signal);

                if matches!(signal, Signal::User1) {
                    crate::s_println!(
                        "sigaction: SIGUSR1 pid={} old={:?}",
                        pid,
                        current_signal_action.handling_type
                    );
                }
            let old_encoded = encode_sigaction(current_signal_action);

            if let Some(decoded) = new_action_decoded {
                if matches!(signal, Signal::User1) {
                    crate::s_println!(
                        "sigaction: SIGUSR1 pid={} new={:?}",
                        pid,
                        decoded.handling_type
                    );
                }
                *current_signal_action = decoded;
            }

            (pid, old_encoded)
        };

        let _ = pid;

        if !old_action.is_null() {
            user_safe::write(old_action, &old_encoded)?;
        }

        Ok(0)
    }
);

define_syscall!(Sigaltstack, |new_stack: u64, old_stack: u64| {
    let new_stack = new_stack as *const LinuxStack;
    let old_stack = old_stack as *mut LinuxStack;

    let mut state = SIGALTSTACK_STATE.lock();

    unsafe {
        if !old_stack.is_null() {
            user_safe::write(old_stack, &*state)?;
        }

        if new_stack.is_null() {
            return Ok(0);
        }

        let new_stack = &*new_stack;
        if (new_stack.ss_flags & !(SS_DISABLE)) != 0 {
            return Err(SyscallError::InvalidArguments);
        }

        if (new_stack.ss_flags & SS_DISABLE) != 0 {
            *state = LinuxStack {
                ss_sp: 0,
                ss_flags: SS_DISABLE,
                ss_size: 0,
            };
            return Ok(0);
        }

        if new_stack.ss_sp == 0 {
            return Err(SyscallError::InvalidArguments);
        }
        if new_stack.ss_size < MINSIGSTKSZ {
            return Err(SyscallError::NoMemory);
        }

        *state = LinuxStack {
            ss_sp: new_stack.ss_sp,
            ss_flags: new_stack.ss_flags & !SS_ONSTACK,
            ss_size: new_stack.ss_size,
        };
    }

    Ok(0)
});

define_syscall!(Kill, |pid: i32, signal: i32| {
    let signal = if signal == 0 {
        None
    } else {
        Some(Signal::try_from(signal as u64).map_err(|_| SyscallError::InvalidArguments)?)
    };

    let current_group = get_current_process().lock().group_id;
    let mut targets = alloc::vec::Vec::new();
    {
        let manager = MANAGER.lock();

        match pid {
            i32::MIN..=-2 => {
                let group = ProcessGroupID((-pid) as u64);
                for process in manager.processes.values() {
                    if process.lock().group_id == group {
                        targets.push(process.clone());
                    }
                }
            }
            -1 => {
                for process in manager.processes.values() {
                    targets.push(process.clone());
                }
            }
            0 => {
                for process in manager.processes.values() {
                    if process.lock().group_id == current_group {
                        targets.push(process.clone());
                    }
                }
            }
            positive => {
                let process = manager
                    .processes
                    .get(&ProcessID(positive as u64))
                    .cloned()
                    .ok_or(SyscallError::NoProcess)?;
                targets.push(process);
            }
        }
    }

    if targets.is_empty() {
        return Err(SyscallError::NoProcess);
    }

    if let Some(signal) = signal {
        for process in targets {
            process.lock().send_signal(signal);
        }
    }

    Ok(0)
});

define_syscall!(Tgkill, |tgid: i32, tid: i32, signal: i32| {
    let signal = Signal::try_from(signal as u64).map_err(|_| SyscallError::InvalidArguments)?;
    let tgid = ProcessID(tgid as u64);
    let tid = crate::thread::misc::ThreadID(tid as u64);

    let thread = THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .threads
        .get(&tid)
        .cloned()
        .ok_or(SyscallError::NoProcess)?;

    let process = thread.lock().parent.clone();
    if process.lock().pid != tgid {
        return Err(SyscallError::NoProcess);
    }

    process.lock().send_signal(signal);
    Ok(0)
});

define_syscall!(SendSignalGroup, |group: ProcessGroupID, signal: Signal| {
    for ele in group.get_processes() {
        ele.lock().send_signal(signal);
    }

    Ok(0)
});

define_syscall!(
    BlockSignals,
    |signals: Signals, old_signals: *mut Signals| {
        let previous = get_current_thread().lock().blocked_signals;
        user_safe::write(old_signals, &previous)?;
        get_current_thread().lock().blocked_signals.insert(signals);
        Ok(0)
    }
);

define_syscall!(
    UnblockSignals,
    |signals: Signals, old_signals: *mut Signals| {
        let previous = get_current_thread().lock().blocked_signals;
        user_safe::write(old_signals, &previous)?;
        get_current_thread().lock().blocked_signals.remove(signals);

        Ok(0)
    }
);

define_syscall!(
    RtSigprocmask,
    |how: i32, set: u64, old_set: *mut u64, sigsetsize: usize| {
        if sigsetsize != size_of::<u64>() {
            return Err(SyscallError::InvalidArguments);
        }

        let current = get_current_thread();
        let mut current = current.lock();
        let set = set as *const u64;

        unsafe {
            if !old_set.is_null() {
                user_safe::write(old_set, &current.blocked_signals.bits())?;
            }

            if !set.is_null() {
                let set = Signals::from_bits_truncate(*set);
                match SigMaskHow::try_from(how).map_err(|_| SyscallError::InvalidArguments)? {
                    SigMaskHow::Block => current.blocked_signals.insert(set),
                    SigMaskHow::Unblock => current.blocked_signals.remove(set),
                    SigMaskHow::SetMask => current.blocked_signals = set,
                }
            }
        }

        Ok(0)
    }
);

define_syscall!(RtSigreturn, {
    get_current_thread().lock().snapshot_state = SnapshotState::Normal;
    get_current_thread().lock().restore_blocked_signals();

    return_to_executor_no_save();
});

define_syscall!(SendSignalToAll, |signal: Signal| {
    for process in MANAGER.lock().processes.values() {
        process.lock().send_signal(signal);
    }

    Ok(0)
});
