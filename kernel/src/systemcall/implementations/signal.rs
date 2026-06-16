use crate::process::group::ProcessGroupID;
use crate::process::manager::MANAGER;
use crate::process::misc::ProcessID;
use crate::signal::action::{SignalHandlingType, Signals};
use crate::systemcall::utils::*;
use crate::thread::extended_state::update_active_user_extended_state_ptr_for_thread;
use crate::thread::get_current_thread;
use crate::thread::misc::{SnapshotState, ThreadID};
use crate::thread::scheduling::return_to_scheduler_no_save;
use crate::thread::thread::LinuxStack;
use crate::thread::yielding::{BlockType, WakeType, block_current_with_sig_check};
use crate::{
    define_syscall,
    memory::user_safe,
    object::{
        FileFlags, Object,
        linux_anon::{SignalfdFlags, SignalfdObject},
        misc::{ObjectRef, get_object_current_process},
    },
    process::misc::{get_process_with_pid, with_current_process},
    process::{FdFlags, manager::get_current_process},
    signal::{
        PendingSignalInfo, SI_QUEUE, SI_TKILL, SigInfo, Signal, UContext, action::SignalAction,
        send_signal_to_process, send_signal_to_process_with_siginfo,
        send_signal_to_thread_with_siginfo,
    },
};
use alloc::vec::Vec;
use bitflags::bitflags;
use core::mem::size_of;
use num_enum::TryFromPrimitive;
use strum::IntoEnumIterator;

const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;
const MINSIGSTKSZ: usize = 2048;

bitflags! {
    #[derive(Clone, Copy, Debug)]
    struct SigActionFlags: u64 {
        const SA_SIGINFO = 0x0000_0004;
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

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxTimespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn encode_sigaction(action: &SignalAction) -> LinuxSigAction {
    let (handler, extra_flags) = match action.handling_type {
        SignalHandlingType::Default => (SIG_DFL, 0),
        SignalHandlingType::Ignore => (SIG_IGN, 0),
        SignalHandlingType::Function1(func) => (func as usize, 0),
        SignalHandlingType::Function2(func) => (func as usize, SigActionFlags::SA_SIGINFO.bits()),
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
                .contains(SigActionFlags::SA_SIGINFO) =>
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

fn read_or_build_signal_info(
    signal: Signal,
    info: *const SigInfo,
    default_code: i32,
) -> SyscallResult<SigInfo> {
    if info.is_null() {
        let current = get_current_process();
        let current = current.lock();
        let mut siginfo =
            SigInfo::for_process_signal(signal, current.pid.0 as i32, current.real_uid);
        siginfo.si_code = default_code;
        return Ok(siginfo);
    }

    let mut siginfo = user_safe::read(info)?;
    siginfo.si_signo = signal as i32;
    if siginfo.si_code == 0 && siginfo.sender_pid() == 0 {
        let current = get_current_process();
        let current = current.lock();
        siginfo = SigInfo::for_process_signal(signal, current.pid.0 as i32, current.real_uid);
        siginfo.si_code = default_code;
    }

    Ok(siginfo)
}

fn linux_timespec_to_ns(timespec: LinuxTimespec) -> Result<u64, SyscallError> {
    if timespec.tv_sec < 0 || timespec.tv_nsec < 0 || timespec.tv_nsec >= 1_000_000_000 {
        return Err(SyscallError::InvalidArguments);
    }

    Ok((timespec.tv_sec as u64).saturating_mul(1_000_000_000) + timespec.tv_nsec as u64)
}

fn dequeue_wait_signal(wait_mask: Signals) -> Option<(Signal, SigInfo)> {
    let thread_ref = get_current_thread();
    let process_ref = thread_ref.lock().parent.clone();

    {
        let mut thread = thread_ref.lock();
        for signal in Signal::iter() {
            let signal_bits = Signals::from(signal);
            if !wait_mask.contains(signal_bits) || !thread.pending_signals.contains(signal_bits) {
                continue;
            }

            thread.pending_signals.remove(signal_bits);
            let siginfo = thread.pending_signal_info[signal.index()]
                .take()
                .unwrap_or_else(|| PendingSignalInfo::for_signal(signal))
                .to_siginfo();
            return Some((signal, siginfo));
        }
    }

    {
        let mut process = process_ref.lock();
        for signal in Signal::iter() {
            let signal_bits = Signals::from(signal);
            if !wait_mask.contains(signal_bits) || !process.pending_signals.contains(signal_bits) {
                continue;
            }

            process.pending_signals.remove(signal_bits);
            let siginfo = process.pending_signal_info[signal.index()]
                .take()
                .unwrap_or_else(|| PendingSignalInfo::for_signal(signal))
                .to_siginfo();
            return Some((signal, siginfo));
        }
    }

    None
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
        let new_action_decoded = if new_action.is_null() {
            None
        } else {
            Some(decode_sigaction(user_safe::read(new_action)?))
        };
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
        let current_thread = get_current_thread();
        let mut thread = current_thread.lock();

        if !old_stack.is_null() {
            user_safe::write(old_stack, &thread.sigaltstack)?;
        }

        if new_stack.is_null() {
            return Ok(0);
        }

        let new_stack = user_safe::read(new_stack)?;
        let new_flags =
            StackFlags::from_bits(new_stack.ss_flags).ok_or(SyscallError::InvalidArguments)?;
        if new_flags.intersects(StackFlags::SS_ONSTACK) {
            return Err(SyscallError::InvalidArguments);
        }

        if new_flags.contains(StackFlags::SS_DISABLE) {
            thread.sigaltstack = LinuxStack {
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

        thread.sigaltstack = LinuxStack {
            ss_sp: new_stack.ss_sp,
            ss_flags: new_flags.bits(),
            ss_size: new_stack.ss_size,
        };

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
            send_signal_to_process(&process, signal);
        }
    }

    Ok(0)
});

define_syscall!(Tgkill, |tgid: i32, tid: i32, signal: i32| {
    let signal = Signal::try_from(signal as u64).map_err(|_| SyscallError::InvalidArguments)?;
    let tgid = ProcessID(tgid as u64);
    let tid = ThreadID(tid as u64);

    let thread = crate::thread::with_thread_manager(|manager| manager.threads.get(&tid).cloned())
        .ok_or(SyscallError::NoProcess)?;

    let process = thread.lock().parent.clone();
    if process.lock().pid != tgid {
        return Err(SyscallError::NoProcess);
    }

    let (sender_pid, sender_uid) = {
        let current = get_current_process();
        let current = current.lock();
        (current.pid.0 as i32, current.real_uid)
    };
    let mut siginfo = SigInfo::for_process_signal(signal, sender_pid, sender_uid);
    siginfo.si_code = SI_TKILL;

    send_signal_to_thread_with_siginfo(&thread, signal, siginfo);
    Ok(0)
});

define_syscall!(
    RtSigqueueinfo,
    |pid: i32, signal: i32, info: *const SigInfo| {
        if pid <= 0 {
            return Err(SyscallError::InvalidArguments);
        }

        let signal = Signal::try_from(signal as u64).map_err(|_| SyscallError::InvalidArguments)?;
        let process = get_process_with_pid(ProcessID(pid as u64))?;
        let siginfo = read_or_build_signal_info(signal, info, SI_QUEUE)?;
        send_signal_to_process_with_siginfo(&process, signal, siginfo);
        Ok(0)
    }
);

define_syscall!(
    PidfdSendSignal,
    |pidfd: ObjectRef, signal: i32, info: *const SigInfo, flags: u32| {
        if flags != 0 {
            return Err(SyscallError::InvalidArguments);
        }

        let pid = pidfd.as_pidfd()?.pid();
        let process = get_process_with_pid(ProcessID(pid))?;
        if signal == 0 {
            if !info.is_null() {
                return Err(SyscallError::InvalidArguments);
            }
            return Ok(0);
        }

        let signal = Signal::try_from(signal as u64).map_err(|_| SyscallError::InvalidArguments)?;
        let siginfo = read_or_build_signal_info(signal, info, SI_QUEUE)?;
        send_signal_to_process_with_siginfo(&process, signal, siginfo);
        Ok(0)
    }
);

define_syscall!(SendSignalGroup, |group: ProcessGroupID, signal: Signal| {
    for ele in group.get_processes() {
        send_signal_to_process(&ele, signal);
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

        let set = if set.is_null() {
            None
        } else {
            Some(Signals::from_bits_truncate(user_safe::read(set)?))
        };

        let old_bits = {
            let current = get_current_thread();
            let mut current = current.lock();
            let old_bits = current.blocked_signals.bits();

            if let Some(set) = set {
                let unmaskable = Signals::from(Signal::SIGKILL) | Signals::from(Signal::SIGSTOP);
                match SigMaskHow::try_from(how).map_err(|_| SyscallError::InvalidArguments)? {
                    SigMaskHow::Block => current.blocked_signals.insert(set - unmaskable),
                    SigMaskHow::Unblock => current.blocked_signals.remove(set),
                    SigMaskHow::SetMask => current.blocked_signals = set - unmaskable,
                }
            }

            old_bits
        };

        if !old_set.is_null() {
            user_safe::write(old_set, &old_bits)?;
        }

        Ok(0)
    }
);

define_syscall!(RtSigpending, |set: *mut u64, sigsetsize: usize| {
    if sigsetsize != size_of::<u64>() {
        return Err(SyscallError::InvalidArguments);
    }
    if set.is_null() {
        return Err(SyscallError::BadAddress);
    }

    let pending = {
        let thread = get_current_thread();
        let thread = thread.lock();
        let process = thread.parent.lock();
        (process.pending_signals | thread.pending_signals).bits()
    };
    user_safe::write(set, &pending)?;
    Ok(0)
});

define_syscall!(
    RtSigtimedwait,
    |set: *const u64, info: *mut SigInfo, timeout: *const LinuxTimespec, sigsetsize: usize| {
        if sigsetsize != size_of::<u64>() {
            return Err(SyscallError::InvalidArguments);
        }
        if set.is_null() {
            return Err(SyscallError::BadAddress);
        }

        let wait_mask = Signals::from_bits_truncate(user_safe::read(set)?);
        let deadline = if timeout.is_null() {
            None
        } else {
            let timeout = user_safe::read(timeout)?;
            Some(crate::misc::time::Time::since_boot().add_ns(linux_timespec_to_ns(timeout)?))
        };

        loop {
            if let Some((signal, siginfo)) = dequeue_wait_signal(wait_mask) {
                if !info.is_null() {
                    user_safe::write(info, &siginfo)?;
                }
                return Ok(signal as usize);
            }

            if let Some(deadline) = deadline
                && crate::misc::time::Time::since_boot() >= deadline
            {
                return Err(SyscallError::TryAgain);
            }

            let block_type = BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline,
            };
            match block_current_with_sig_check(block_type) {
                Ok(()) => {}
                Err(_) => {
                    if let Some((signal, siginfo)) = dequeue_wait_signal(wait_mask) {
                        if !info.is_null() {
                            user_safe::write(info, &siginfo)?;
                        }
                        return Ok(signal as usize);
                    }
                    return Err(SyscallError::Interrupted);
                }
            }
        }
    }
);

define_syscall!(RtSigreturn, {
    let current = get_current_thread();
    let mut thread = current.lock();
    thread.snapshot_state = SnapshotState::Normal;
    thread.restore_blocked_signals();
    update_active_user_extended_state_ptr_for_thread(&mut thread);
    drop(thread);

    return_to_scheduler_no_save();
});

define_syscall!(SendSignalToAll, |signal: Signal| {
    for process in MANAGER.lock().processes.values() {
        send_signal_to_process(process, signal);
    }

    Ok(0)
});

#[cfg(test)]
mod tests {
    use crate::{
        misc::signal::send_signal_to_process_with_siginfo,
        object::FileFlags,
        process::{
            FdFlags, Process,
            manager::{MANAGER, get_current_process},
            misc::ProcessID,
        },
        signal::{SigInfo, Signal, Signals},
        systemcall::test::{
            TestLinuxItimerval, TestLinuxSigAction, TestLinuxStack, TestLinuxTimespec,
            TestWaitidSigInfo, assert_fd_flags, assert_object_flags, close_test_fd, expect_fd,
        },
        systemcall::test_helpers::{
            SyscallArgs, allocate_user_test_page, assert_linux_layout, expect_errno, expect_ok,
            read_user_value, write_user_value,
        },
        systemcall::{
            implementations::{
                Kill, Nanosleep, Pause, Read, RtSigaction, RtSigpending, RtSigprocmask,
                RtSigqueueinfo, RtSigsuspend, RtSigtimedwait, Setitimer, Sigaltstack, Signalfd4,
                Tgkill,
            },
            utils::SyscallError,
        },
        thread::extended_state::active_user_extended_state_ptr,
    };

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct TestLinuxSignalfdSiginfo {
        ssi_signo: u32,
        ssi_errno: i32,
        ssi_code: i32,
        ssi_pid: u32,
        ssi_uid: u32,
        ssi_fd: i32,
        ssi_tid: u32,
        ssi_band: u32,
        ssi_overrun: u32,
        ssi_trapno: u32,
        ssi_status: i32,
        ssi_int: i32,
        ssi_ptr: u64,
        ssi_utime: u64,
        ssi_stime: u64,
        ssi_addr: u64,
        ssi_addr_lsb: u16,
        __pad2: u16,
        ssi_syscall: i32,
        ssi_call_addr: u64,
        ssi_arch: u32,
        __pad: [u8; 28],
    }

    crate::test!(
        signalfd_syscalls,
        "signalfd syscalls follow linux rules",
        signalfd_syscalls_follow_linux_rules
    );
    crate::test!(
        sleep_and_signal_mask_syscalls,
        "nanosleep setitimer and rt_sigsuspend follow linux rules",
        sleep_and_signal_mask_syscalls_follow_linux_rules
    );
    crate::test!(
        process_and_signal_transition_helpers,
        "signal return and process transition helpers follow linux rules",
        process_and_signal_transition_helpers_follow_linux_rules
    );

    fn sleep_and_signal_mask_syscalls_follow_linux_rules() {
        const SIG_BLOCK: u64 = 0;
        const SIG_UNBLOCK: u64 = 1;
        const SIG_SETMASK: u64 = 2;
        const SS_ONSTACK: i32 = 1;
        const SS_DISABLE: i32 = 2;
        const SA_SIGINFO: u64 = 0x0000_0004;
        const SI_QUEUE: i32 = -1;
        const MINSIGSTKSZ: usize = 2048;

        assert_linux_layout::<TestLinuxStack>(24, 8);
        assert_linux_layout::<TestLinuxSigAction>(32, 8);

        let page = allocate_user_test_page();
        write_user_value(
            page,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        );
        expect_ok(
            SyscallArgs::new([page, page + 32, 0, 0, 0, 0]).call::<Nanosleep>(),
            0,
        );
        assert_eq!(read_user_value::<TestLinuxTimespec>(page + 32).tv_nsec, 0);
        write_user_value(
            page,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
        );
        expect_errno(
            SyscallArgs::new([page, 0, 0, 0, 0, 0]).call::<Nanosleep>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([1, 0, 0, 0, 0, 0]).call::<Nanosleep>(),
            SyscallError::BadAddress,
        );

        write_user_value(page + 64, &TestLinuxItimerval::default());
        expect_ok(
            SyscallArgs::new([0, page + 64, page + 96, 0, 0, 0]).call::<Setitimer>(),
            0,
        );
        assert_eq!(
            read_user_value::<TestLinuxItimerval>(page + 96)
                .it_value
                .tv_sec,
            0
        );
        expect_errno(
            SyscallArgs::new([99, page + 64, 0, 0, 0, 0]).call::<Setitimer>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Setitimer>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([0, 1, 0, 0, 0, 0]).call::<Setitimer>(),
            SyscallError::BadAddress,
        );

        let thread = crate::thread::get_current_thread();
        let saved_mask = thread.lock().blocked_signals;
        write_user_value(page + 128, &Signal::SIGUSR1.mask());
        expect_ok(
            SyscallArgs::new([SIG_BLOCK, page + 128, page + 136, 8, 0, 0]).call::<RtSigprocmask>(),
            0,
        );
        assert_eq!(read_user_value::<u64>(page + 136), saved_mask.bits());
        assert!(
            crate::thread::get_current_thread()
                .lock()
                .blocked_signals
                .contains(Signals::from(Signal::SIGUSR1))
        );

        write_user_value(
            page + 144,
            &(Signal::SIGKILL.mask() | Signal::SIGSTOP.mask()),
        );
        expect_ok(
            SyscallArgs::new([SIG_BLOCK, page + 144, 0, 8, 0, 0]).call::<RtSigprocmask>(),
            0,
        );
        let blocked = crate::thread::get_current_thread().lock().blocked_signals;
        assert!(!blocked.contains(Signals::from(Signal::SIGKILL)));
        assert!(!blocked.contains(Signals::from(Signal::SIGSTOP)));

        expect_ok(
            SyscallArgs::new([SIG_UNBLOCK, page + 128, 0, 8, 0, 0]).call::<RtSigprocmask>(),
            0,
        );
        assert!(
            !crate::thread::get_current_thread()
                .lock()
                .blocked_signals
                .contains(Signals::from(Signal::SIGUSR1))
        );

        write_user_value(page + 152, &Signal::SIGTERM.mask());
        expect_ok(
            SyscallArgs::new([SIG_SETMASK, page + 152, 0, 8, 0, 0]).call::<RtSigprocmask>(),
            0,
        );
        assert_eq!(
            crate::thread::get_current_thread()
                .lock()
                .blocked_signals
                .bits(),
            Signal::SIGTERM.mask()
        );
        expect_errno(
            SyscallArgs::new([99, page + 152, 0, 8, 0, 0]).call::<RtSigprocmask>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([SIG_BLOCK, 1, 0, 8, 0, 0]).call::<RtSigprocmask>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([SIG_BLOCK, page + 152, 0, 4, 0, 0]).call::<RtSigprocmask>(),
            SyscallError::InvalidArguments,
        );

        let current = get_current_process();
        let current_group = current.lock().group_id;
        let peer = Process::empty();
        let peer_pid = {
            let mut peer = peer.lock();
            peer.pid = ProcessID::new();
            peer.group_id = current_group;
            peer.parent = Some(current.clone());
            peer.pid.0 as i32
        };
        MANAGER
            .lock()
            .processes
            .insert(ProcessID(peer_pid as u64), peer.clone());

        expect_ok(
            SyscallArgs::new([peer_pid as u64, 0, 0, 0, 0, 0]).call::<Kill>(),
            0,
        );
        expect_errno(
            SyscallArgs::new([peer_pid as u64, 65, 0, 0, 0, 0]).call::<Kill>(),
            SyscallError::InvalidArguments,
        );
        expect_ok(
            SyscallArgs::new([u64::MAX, 0, 0, 0, 0, 0]).call::<Kill>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([0, Signal::SIGUSR1 as u64, 0, 0, 0, 0]).call::<Kill>(),
            0,
        );
        assert!(
            current
                .lock()
                .pending_signals
                .contains(Signals::from(Signal::SIGUSR1))
        );
        assert!(
            peer.lock()
                .pending_signals
                .contains(Signals::from(Signal::SIGUSR1))
        );
        current
            .lock()
            .pending_signals
            .remove(Signals::from(Signal::SIGUSR1));
        peer.lock()
            .pending_signals
            .remove(Signals::from(Signal::SIGUSR1));

        send_signal_to_process_with_siginfo(
            &current,
            Signal::SIGUSR2,
            SigInfo::for_signal(Signal::SIGUSR2),
        );
        expect_errno(
            SyscallArgs::new([0, 0, 0, 0, 0, 0]).call::<Pause>(),
            SyscallError::Interrupted,
        );
        {
            let mut current = current.lock();
            current
                .pending_signals
                .remove(Signals::from(Signal::SIGUSR2));
            current.pending_signal_info[Signal::SIGUSR2.index()] = None;
        }

        expect_ok(
            SyscallArgs::new([0, page + 192, 0, 0, 0, 0]).call::<Sigaltstack>(),
            0,
        );
        assert_eq!(
            read_user_value::<TestLinuxStack>(page + 192).ss_flags,
            SS_DISABLE
        );
        let altstack = TestLinuxStack {
            ss_sp: page + 4096,
            ss_flags: 0,
            ss_size: MINSIGSTKSZ,
        };
        write_user_value(page + 224, &altstack);
        expect_ok(
            SyscallArgs::new([page + 224, page + 256, 0, 0, 0, 0]).call::<Sigaltstack>(),
            0,
        );
        assert_eq!(
            read_user_value::<TestLinuxStack>(page + 256).ss_flags,
            SS_DISABLE
        );
        expect_ok(
            SyscallArgs::new([0, page + 288, 0, 0, 0, 0]).call::<Sigaltstack>(),
            0,
        );
        assert_eq!(
            read_user_value::<TestLinuxStack>(page + 288).ss_sp,
            altstack.ss_sp
        );
        assert_eq!(
            read_user_value::<TestLinuxStack>(page + 288).ss_size,
            MINSIGSTKSZ
        );
        write_user_value(
            page + 320,
            &TestLinuxStack {
                ss_sp: page + 8192,
                ss_flags: SS_DISABLE,
                ss_size: 9999,
            },
        );
        expect_ok(
            SyscallArgs::new([page + 320, 0, 0, 0, 0, 0]).call::<Sigaltstack>(),
            0,
        );
        expect_ok(
            SyscallArgs::new([0, page + 352, 0, 0, 0, 0]).call::<Sigaltstack>(),
            0,
        );
        let disabled_stack = read_user_value::<TestLinuxStack>(page + 352);
        assert_eq!(disabled_stack.ss_flags, SS_DISABLE);
        assert_eq!(disabled_stack.ss_sp, 0);
        assert_eq!(disabled_stack.ss_size, 0);
        write_user_value(
            page + 384,
            &TestLinuxStack {
                ss_sp: page + 12288,
                ss_flags: SS_ONSTACK,
                ss_size: MINSIGSTKSZ,
            },
        );
        expect_errno(
            SyscallArgs::new([page + 384, 0, 0, 0, 0, 0]).call::<Sigaltstack>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            page + 416,
            &TestLinuxStack {
                ss_sp: 0,
                ss_flags: 0,
                ss_size: MINSIGSTKSZ,
            },
        );
        expect_errno(
            SyscallArgs::new([page + 416, 0, 0, 0, 0, 0]).call::<Sigaltstack>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            page + 448,
            &TestLinuxStack {
                ss_sp: page + 16384,
                ss_flags: 0,
                ss_size: MINSIGSTKSZ - 1,
            },
        );
        expect_errno(
            SyscallArgs::new([page + 448, 0, 0, 0, 0, 0]).call::<Sigaltstack>(),
            SyscallError::NoMemory,
        );

        extern "C" fn test_siginfo_handler(
            _: i32,
            _: *const SigInfo,
            _: *const crate::signal::UContext,
        ) {
        }
        let new_action = TestLinuxSigAction {
            handler: test_siginfo_handler as *const () as usize,
            flags: SA_SIGINFO,
            restorer: 0x1234_5678_9abc_def0usize,
            mask: Signal::SIGUSR1.mask(),
        };
        write_user_value(page + 480, &new_action);
        expect_ok(
            SyscallArgs::new([Signal::SIGUSR2 as u64, page + 480, page + 544, 8, 0, 0])
                .call::<RtSigaction>(),
            0,
        );
        let old_action = read_user_value::<TestLinuxSigAction>(page + 544);
        assert_eq!(old_action.handler, 0);
        assert_eq!(old_action.flags & SA_SIGINFO, 0);
        assert_eq!(old_action.mask, 0);
        expect_ok(
            SyscallArgs::new([Signal::SIGUSR2 as u64, 0, page + 576, 8, 0, 0])
                .call::<RtSigaction>(),
            0,
        );
        let installed_action = read_user_value::<TestLinuxSigAction>(page + 576);
        assert_eq!(
            installed_action.handler,
            test_siginfo_handler as *const () as usize
        );
        assert_ne!(installed_action.flags & SA_SIGINFO, 0);
        assert_eq!(installed_action.restorer, new_action.restorer);
        assert_eq!(installed_action.mask, Signal::SIGUSR1.mask());
        expect_errno(
            SyscallArgs::new([Signal::SIGUSR2 as u64, 1, 0, 8, 0, 0]).call::<RtSigaction>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([Signal::SIGUSR2 as u64, 0, 1, 8, 0, 0]).call::<RtSigaction>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([Signal::SIGUSR2 as u64, 0, 0, 4, 0, 0]).call::<RtSigaction>(),
            SyscallError::InvalidArguments,
        );

        let queued_process = Process::empty();
        let queued_pid = {
            let mut process = queued_process.lock();
            process.pid = ProcessID::new();
            process.parent = Some(current.clone());
            process.group_id = current_group;
            process.pid.0 as i32
        };
        MANAGER
            .lock()
            .processes
            .insert(ProcessID(queued_pid as u64), queued_process.clone());
        let mut queued_siginfo = SigInfo::for_process_signal(Signal::SIGTERM, 77, 88);
        queued_siginfo.si_code = SI_QUEUE;
        write_user_value(page + 768, &queued_siginfo);
        expect_ok(
            SyscallArgs::new([
                queued_pid as u64,
                Signal::SIGTERM as u64,
                page + 768,
                0,
                0,
                0,
            ])
            .call::<RtSigqueueinfo>(),
            0,
        );
        {
            let queued_process = queued_process.lock();
            assert!(
                queued_process
                    .pending_signals
                    .contains(Signals::from(Signal::SIGTERM))
            );
            let pending = queued_process.pending_signal_info[Signal::SIGTERM.index()]
                .expect("sigqueueinfo should store pending siginfo");
            assert_eq!(pending.si_code, SI_QUEUE);
            assert_eq!(pending.si_pid, 77);
            assert_eq!(pending.si_uid, 88);
        }
        expect_errno(
            SyscallArgs::new([0, Signal::SIGTERM as u64, 0, 0, 0, 0]).call::<RtSigqueueinfo>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([queued_pid as u64, 65, 0, 0, 0, 0]).call::<RtSigqueueinfo>(),
            SyscallError::InvalidArguments,
        );
        let tgkill_thread = crate::thread::thread::Thread::empty();
        let target_tid = {
            let mut thread = tgkill_thread.lock();
            thread.parent = current.clone();
            thread.id = crate::thread::misc::ThreadID::new();
            thread.id.0 as u64
        };
        crate::thread::THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .threads
            .insert(
                crate::thread::misc::ThreadID(target_tid),
                tgkill_thread.clone(),
            );
        current
            .lock()
            .threads
            .push(alloc::sync::Arc::downgrade(&tgkill_thread));
        let target_tgid = current.lock().pid.0 as u64;
        expect_ok(
            SyscallArgs::new([target_tgid, target_tid, Signal::SIGUSR1 as u64, 0, 0, 0])
                .call::<Tgkill>(),
            0,
        );
        {
            let thread = tgkill_thread.lock();
            assert!(
                thread
                    .pending_signals
                    .contains(Signals::from(Signal::SIGUSR1))
            );
            let pending = thread.pending_signal_info[Signal::SIGUSR1.index()]
                .expect("tgkill should queue thread siginfo");
            assert_eq!(pending.si_code, crate::misc::signal::SI_TKILL);
            assert_eq!(pending.si_pid, current.lock().pid.0 as i32);
            assert_eq!(pending.si_uid, current.lock().real_uid);
        }
        {
            let mut thread = tgkill_thread.lock();
            thread
                .pending_signals
                .remove(Signals::from(Signal::SIGUSR1));
            thread.pending_signal_info[Signal::SIGUSR1.index()] = None;
        }
        expect_errno(
            SyscallArgs::new([u64::MAX, target_tid, Signal::SIGUSR1 as u64, 0, 0, 0])
                .call::<Tgkill>(),
            SyscallError::NoProcess,
        );
        expect_errno(
            SyscallArgs::new([target_tgid, u64::MAX, Signal::SIGUSR1 as u64, 0, 0, 0])
                .call::<Tgkill>(),
            SyscallError::NoProcess,
        );
        expect_errno(
            SyscallArgs::new([target_tgid, target_tid, 65, 0, 0, 0]).call::<Tgkill>(),
            SyscallError::InvalidArguments,
        );
        crate::thread::THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .threads
            .remove(&crate::thread::misc::ThreadID(target_tid));
        current.lock().threads.retain(|candidate| {
            candidate
                .upgrade()
                .is_some_and(|thread| thread.lock().id.0 != target_tid)
        });

        {
            let thread_ref = crate::thread::get_current_thread();
            let mut thread = thread_ref.lock();
            thread.pending_signals = Signals::empty();
            thread.pending_signal_info.fill(None);
        }
        {
            let thread_parent = crate::thread::get_current_thread().lock().parent.clone();
            let mut current = thread_parent.lock();
            current.pending_signals = Signals::empty();
            current.pending_signal_info.fill(None);
        }

        let mut timed_siginfo = SigInfo::for_process_signal(Signal::SIGUSR1, 123, 456);
        timed_siginfo.si_code = SI_QUEUE;
        let thread_parent = crate::thread::get_current_thread().lock().parent.clone();
        send_signal_to_process_with_siginfo(&thread_parent, Signal::SIGUSR1, timed_siginfo);
        assert_eq!(
            crate::thread::get_current_thread()
                .lock()
                .pending_signals
                .bits(),
            0
        );
        assert_eq!(
            thread_parent.lock().pending_signals.bits(),
            Signal::SIGUSR1.mask()
        );
        send_signal_to_process_with_siginfo(&thread_parent, Signal::SIGUSR1, timed_siginfo);
        write_user_value(page + 608, &Signal::SIGUSR1.mask());
        expect_ok(
            SyscallArgs::new([page + 608, page + 640, 0, 8, 0, 0]).call::<RtSigtimedwait>(),
            Signal::SIGUSR1 as usize,
        );
        let waited_info = read_user_value::<TestWaitidSigInfo>(page + 640);
        assert_eq!(waited_info.si_signo, Signal::SIGUSR1 as i32);
        assert_eq!(waited_info.si_code, SI_QUEUE);
        assert_eq!(waited_info.si_pid, 123);
        assert_eq!(waited_info.si_uid, 456);
        assert!(
            !current
                .lock()
                .pending_signals
                .contains(Signals::from(Signal::SIGUSR1))
        );
        write_user_value(
            page + 736,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        );
        expect_errno(
            SyscallArgs::new([page + 608, page + 640, page + 736, 8, 0, 0])
                .call::<RtSigtimedwait>(),
            SyscallError::TryAgain,
        );
        expect_errno(
            SyscallArgs::new([0, page + 640, 0, 8, 0, 0]).call::<RtSigtimedwait>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([page + 608, page + 640, page + 736, 4, 0, 0])
                .call::<RtSigtimedwait>(),
            SyscallError::InvalidArguments,
        );
        write_user_value(
            page + 736,
            &TestLinuxTimespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
        );
        expect_errno(
            SyscallArgs::new([page + 608, page + 640, page + 736, 8, 0, 0])
                .call::<RtSigtimedwait>(),
            SyscallError::InvalidArguments,
        );

        {
            let mut current = thread.lock();
            current
                .pending_signals
                .insert(Signals::from(Signal::SIGUSR1));
            current
                .parent
                .lock()
                .pending_signals
                .insert(Signals::from(Signal::SIGTERM));
        }
        expect_ok(
            SyscallArgs::new([page + 168, 8, 0, 0, 0, 0]).call::<RtSigpending>(),
            0,
        );
        let pending = read_user_value::<u64>(page + 168);
        assert_ne!(pending & Signal::SIGUSR1.mask(), 0);
        assert_ne!(pending & Signal::SIGTERM.mask(), 0);
        expect_errno(
            SyscallArgs::new([0, 8, 0, 0, 0, 0]).call::<RtSigpending>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([page + 168, 4, 0, 0, 0, 0]).call::<RtSigpending>(),
            SyscallError::InvalidArguments,
        );
        {
            let mut current = thread.lock();
            current
                .pending_signals
                .remove(Signals::from(Signal::SIGUSR1));
            current
                .parent
                .lock()
                .pending_signals
                .remove(Signals::from(Signal::SIGTERM));
        }

        write_user_value(page + 160, &Signal::SIGUSR1.mask());
        expect_errno(
            SyscallArgs::new([page + 160, 4, 0, 0, 0, 0]).call::<RtSigsuspend>(),
            SyscallError::InvalidArguments,
        );
        expect_errno(
            SyscallArgs::new([0, 8, 0, 0, 0, 0]).call::<RtSigsuspend>(),
            SyscallError::BadAddress,
        );
        expect_errno(
            SyscallArgs::new([1, 8, 0, 0, 0, 0]).call::<RtSigsuspend>(),
            SyscallError::BadAddress,
        );
        MANAGER
            .lock()
            .processes
            .remove(&ProcessID(queued_pid as u64));
        MANAGER.lock().processes.remove(&ProcessID(peer_pid as u64));
        crate::thread::get_current_thread().lock().blocked_signals = saved_mask;
    }

    fn process_and_signal_transition_helpers_follow_linux_rules() {
        let thread = crate::thread::get_current_thread();
        {
            let mut thread = thread.lock();
            let normal_ptr = thread.snapshot.extended_state.active_ptr();
            thread.snapshot_state = crate::thread::misc::SnapshotState::SignalHandler;
            let signal_ptr = thread.sig_handler_snapshot.extended_state.active_ptr();
            thread.blocked_signals = Signals::from(Signal::SIGTERM);
            thread
                .saved_blocked_signals
                .push(Signals::from(Signal::SIGUSR1));
            crate::thread::extended_state::update_active_user_extended_state_ptr_for_thread(
                &mut thread,
            );
            assert_eq!(active_user_extended_state_ptr(), signal_ptr);
            thread.restore_blocked_signals();
            assert_eq!(thread.blocked_signals.bits(), Signal::SIGUSR1.mask());
            thread.snapshot_state = crate::thread::misc::SnapshotState::Normal;
            crate::thread::extended_state::update_active_user_extended_state_ptr_for_thread(
                &mut thread,
            );
            assert_eq!(active_user_extended_state_ptr(), normal_ptr);
            thread.saved_blocked_signals.clear();
        }
    }

    fn signalfd_syscalls_follow_linux_rules() {
        const SFD_NONBLOCK: u64 = 0o4_000;
        const SFD_CLOEXEC: u64 = 0o2_000_000;

        assert_linux_layout::<TestLinuxSignalfdSiginfo>(128, 8);

        let sigmask_user = allocate_user_test_page();
        write_user_value(sigmask_user, &Signal::SIGUSR1.mask());
        let signalfd = expect_fd(
            SyscallArgs::new([
                (-1i32) as u64,
                sigmask_user,
                core::mem::size_of::<u64>() as u64,
                SFD_NONBLOCK | SFD_CLOEXEC,
                0,
                0,
            ])
            .call::<Signalfd4>(),
        );
        assert_fd_flags(signalfd, FdFlags::CLOEXEC);
        assert_object_flags(signalfd, FileFlags::NONBLOCK);
        let siginfo_buf = allocate_user_test_page();
        expect_errno(
            SyscallArgs::new([(-1i32) as u64, sigmask_user, 4, 0, 0, 0]).call::<Signalfd4>(),
            SyscallError::InvalidArguments,
        );

        let mut siginfo: SigInfo = unsafe { core::mem::zeroed() };
        siginfo.si_signo = Signal::SIGUSR1 as i32;
        siginfo.si_errno = 123;
        siginfo.si_code = -6;
        let process = get_current_process();
        send_signal_to_process_with_siginfo(&process, Signal::SIGUSR1, siginfo);
        expect_ok(
            SyscallArgs::new([signalfd as u64, siginfo_buf, 128, 0, 0, 0]).call::<Read>(),
            128,
        );
        let signalfd_info = read_user_value::<TestLinuxSignalfdSiginfo>(siginfo_buf);
        assert_eq!(signalfd_info.ssi_signo, Signal::SIGUSR1 as u32);
        assert_eq!(signalfd_info.ssi_errno, 123);
        assert_eq!(signalfd_info.ssi_code, -6);
        assert_eq!(signalfd_info.ssi_pid, process.lock().pid.0 as u32);

        write_user_value(sigmask_user, &Signal::SIGTERM.mask());
        expect_ok(
            SyscallArgs::new([
                signalfd as u64,
                sigmask_user,
                core::mem::size_of::<u64>() as u64,
                0,
                0,
                0,
            ])
            .call::<Signalfd4>(),
            signalfd,
        );
        expect_errno(
            SyscallArgs::new([signalfd as u64, siginfo_buf, 127, 0, 0, 0]).call::<Read>(),
            SyscallError::InvalidArguments,
        );

        close_test_fd(signalfd);
    }
}
