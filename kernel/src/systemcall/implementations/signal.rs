use crate::process::group::ProcessGroupID;
use crate::process::manager::MANAGER;
use crate::process::misc::ProcessID;
use crate::signal::action::{SignalHandlingType, Signals};
use crate::systemcall::utils::*;
use crate::thread::misc::{SnapshotState, ThreadID};
use crate::thread::scheduling::return_to_executor_no_save;
use crate::thread::{THREAD_MANAGER, get_current_thread};
use crate::{
    define_syscall,
    memory::user_safe,
    object::{
        FileFlags, Object,
        linux_anon::{SignalfdFlags, SignalfdObject},
        misc::{ObjectRef, get_object_current_process},
    },
    process::misc::with_current_process,
    process::{FdFlags, manager::get_current_process},
    signal::{SigInfo, Signal, UContext, action::SignalAction},
};
use alloc::vec::Vec;
use bitflags::bitflags;
use core::mem::size_of;
use num_enum::TryFromPrimitive;
use spin::Mutex;

const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;
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
    ss_flags: StackFlags::SS_DISABLE.bits(),
    ss_size: 0,
});

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct SigActionFlags: u64 {
        const SIGINFO = 0x0000_0004;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct StackFlags: i32 {
        const SS_ONSTACK = 1;
        const SS_DISABLE = 2;
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
        unsafe {
            SignalHandlingType::Function2(core::mem::transmute::<
                usize,
                extern "C" fn(i32, *const SigInfo, *const UContext),
            >(handler))
        },
        handler => unsafe {
            SignalHandlingType::Function1(core::mem::transmute::<usize, extern "C" fn(i32)>(
                handler,
            ))
        },
    };

    SignalAction {
        handling_type,
        sig_handler_ignored_sigs: Signals::from_bits_truncate(action.mask),
        flags: action.flags,
        restorer: action.restorer,
    }
}

define_syscall!(
    Signalfd4,
    |fd: i32, mask: *const u64, sigsetsize: usize, flags: SignalfdFlags| {
        if sigsetsize != size_of::<u64>() {
            return Err(SyscallError::InvalidArguments);
        }

        let mask = user_safe::read(mask)?;

        if fd == -1 {
            let signalfd = SignalfdObject::new(get_current_process().lock().pid.0, mask, flags);
            let signalfd_ref: ObjectRef = signalfd;
            let fd_flags = if flags.contains(SignalfdFlags::SFD_CLOEXEC) {
                FdFlags::CLOEXEC
            } else {
                FdFlags::empty()
            };
            return Ok(with_current_process(|process| {
                process.push_object_with_flags(signalfd_ref, fd_flags)
            }));
        }

        let signalfd = get_object_current_process(fd as u64)
            .map_err(SyscallError::from)?
            .as_signalfd()?;
        signalfd.set_mask(mask);

        let file_flags = if flags.contains(SignalfdFlags::SFD_NONBLOCK) {
            FileFlags::NONBLOCK
        } else {
            FileFlags::empty()
        };
        signalfd
            .clone()
            .set_flags(file_flags)
            .map_err(SyscallError::from)?;
        let fd_flags = if flags.contains(SignalfdFlags::SFD_CLOEXEC) {
            FdFlags::CLOEXEC
        } else {
            FdFlags::empty()
        };
        with_current_process(|process| process.set_fd_flags(fd as usize, fd_flags))?;

        Ok(fd as usize)
    }
);

define_syscall!(
    RtSigaction,
    |signal: i32,
     new_action: *const LinuxSigAction,
     old_action: *mut LinuxSigAction,
     sigsetsize: usize| {
        if sigsetsize != size_of::<u64>() {
            return Err(SyscallError::InvalidArguments);
        }

        let signal = Signal::try_from(signal as u64).map_err(|_| SyscallError::InvalidArguments)?;
        let new_action_decoded =
            unsafe { (!new_action.is_null()).then(|| decode_sigaction(*new_action)) };
        let old_encoded = {
            let process = get_current_process();
            let mut process = process.lock();
            let current_signal_action = process.get_signal_action(signal);
            let old_encoded = encode_sigaction(current_signal_action);

            if let Some(decoded) = new_action_decoded {
                *current_signal_action = decoded;
            }

            old_encoded
        };

        if !old_action.is_null() {
            user_safe::write(old_action, &old_encoded)?;
        }

        Ok(0)
    }
);

define_syscall!(
    Sigaltstack,
    |new_stack: *const LinuxStack, old_stack: *mut LinuxStack| {
        let mut state = SIGALTSTACK_STATE.lock();

        unsafe {
            if !old_stack.is_null() {
                user_safe::write(old_stack, &*state)?;
            }

            if new_stack.is_null() {
                return Ok(0);
            }

            let new_stack = &*new_stack;
            let new_flags =
                StackFlags::from_bits(new_stack.ss_flags).ok_or(SyscallError::InvalidArguments)?;
            if new_flags.intersects(StackFlags::SS_ONSTACK) {
                return Err(SyscallError::InvalidArguments);
            }

            if new_flags.contains(StackFlags::SS_DISABLE) {
                *state = LinuxStack {
                    ss_sp: 0,
                    ss_flags: StackFlags::SS_DISABLE.bits(),
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
                ss_flags: new_flags.bits(),
                ss_size: new_stack.ss_size,
            };
        }

        Ok(0)
    }
);

define_syscall!(Kill, |pid: i32, signal: i32| {
    let signal = if signal == 0 {
        None
    } else {
        Some(Signal::try_from(signal as u64).map_err(|_| SyscallError::InvalidArguments)?)
    };

    let current_group = get_current_process().lock().group_id;
    let mut targets = Vec::new();
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
    let tid = ThreadID(tid as u64);

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

define_syscall!(
    PidfdSendSignal,
    |pidfd: ObjectRef, signal: i32, info: *const u8, flags: u32| {
        if !info.is_null() || flags != 0 {
            return Err(SyscallError::NoSyscall);
        }

        let pid = pidfd.as_pidfd()?.pid();
        Kill::handle_call(pid, signal as u64, 0, 0, 0, 0)
    }
);

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
    |how: i32, set: *const u64, old_set: *mut u64, sigsetsize: usize| {
        if sigsetsize != size_of::<u64>() {
            return Err(SyscallError::InvalidArguments);
        }

        let current = get_current_thread();
        let mut current = current.lock();

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
