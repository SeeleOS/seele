use crate::{
    misc::signal::Signal,
    process::{
        ProcessRef,
        manager::get_current_process,
        misc::{ProcessID, get_process_with_pid},
        wait::{ProcessWaitEvent, report_wait_event},
    },
    systemcall::utils::{SyscallError, SyscallResult},
    thread::yielding::{BlockType, block_current},
};

pub const PTRACE_O_TRACESYSGOOD: u32 = 0x0000_0001;
pub const PTRACE_O_TRACEEXEC: u32 = 0x0000_0010;
pub const PTRACE_EVENT_EXEC: u16 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PtraceResumeMode {
    #[default]
    Stopped,
    Continue,
    Syscall,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PtraceState {
    pub tracer: Option<ProcessID>,
    pub options: u32,
    pub resume_mode: PtraceResumeMode,
    pub last_stop_status: i32,
    pub last_stop_kind: PtraceStopKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PtraceStopKind {
    #[default]
    None,
    Signal,
    SyscallEnter,
    SyscallExit,
    Exec,
}

fn stop_status(signal: i32) -> i32 {
    ((signal & 0xff) << 8) | 0x7f
}

fn exec_event_status() -> i32 {
    (((PTRACE_EVENT_EXEC as i32) << 8) | Signal::SIGTRAP as i32) << 8 | 0x7f
}

fn tracer_pid_of(process: &ProcessRef) -> Option<ProcessID> {
    process.lock().ptrace.tracer
}

pub fn traceme_current() -> SyscallResult<()> {
    let current = get_current_process();
    let Some(parent) = current.lock().parent.clone() else {
        return Err(SyscallError::NoProcess);
    };
    let parent_pid = parent.lock().pid;

    let mut current = current.lock();
    if current.ptrace.tracer.is_some() {
        return Err(SyscallError::PermissionDenied);
    }
    current.ptrace.tracer = Some(parent_pid);
    current.ptrace.resume_mode = PtraceResumeMode::Continue;
    Ok(())
}

pub fn seize(process: &ProcessRef, tracer_pid: ProcessID, options: u32) -> SyscallResult<()> {
    let mut process = process.lock();
    if process.pid == tracer_pid {
        return Err(SyscallError::PermissionDenied);
    }
    if let Some(existing) = process.ptrace.tracer
        && existing != tracer_pid
    {
        return Err(SyscallError::PermissionDenied);
    }
    process.ptrace.tracer = Some(tracer_pid);
    process.ptrace.options = options;
    process.ptrace.resume_mode = PtraceResumeMode::Continue;
    Ok(())
}

pub fn ensure_traced_by(process: &ProcessRef, tracer_pid: ProcessID) -> SyscallResult<()> {
    if tracer_pid_of(process) == Some(tracer_pid) {
        Ok(())
    } else {
        Err(SyscallError::PermissionDenied)
    }
}

pub fn set_options(process: &ProcessRef, tracer_pid: ProcessID, options: u32) -> SyscallResult<()> {
    ensure_traced_by(process, tracer_pid)?;
    process.lock().ptrace.options = options;
    Ok(())
}

pub fn resume(
    process: &ProcessRef,
    tracer_pid: ProcessID,
    mode: PtraceResumeMode,
) -> SyscallResult<()> {
    ensure_traced_by(process, tracer_pid)?;
    {
        let mut process = process.lock();
        process.ptrace.resume_mode = mode;
        process.ptrace.last_stop_status = 0;
        process.ptrace.last_stop_kind = PtraceStopKind::None;
    }

    let thread_id = {
        let process = process.lock();
        process
            .threads
            .iter()
            .find_map(|thread| thread.upgrade().map(|thread| thread.lock().id))
            .ok_or(SyscallError::NoProcess)?
    };
    crate::thread::with_thread_manager(|manager| manager.wake_thread_by_id(thread_id));
    Ok(())
}

pub fn get_traced_process(pid: i32, tracer_pid: ProcessID) -> SyscallResult<ProcessRef> {
    if pid <= 0 {
        return Err(SyscallError::InvalidArguments);
    }
    let process = get_process_with_pid(ProcessID(pid as u64))?;
    ensure_traced_by(&process, tracer_pid)?;
    Ok(process)
}

pub fn report_signal_stop(process: &ProcessRef, signal: Signal) {
    let ptrace = tracer_pid_of(process).is_some();
    report_wait_event(
        process,
        ProcessWaitEvent::Stopped {
            status: stop_status(signal as i32),
            ptrace,
        },
    );
    if ptrace {
        let mut process = process.lock();
        process.ptrace.last_stop_status = stop_status(signal as i32);
        process.ptrace.last_stop_kind = PtraceStopKind::Signal;
    }
}

fn stop_current_with_status(status: i32, kind: PtraceStopKind) {
    let current = get_current_process();
    {
        let mut current_locked = current.lock();
        current_locked.ptrace.resume_mode = PtraceResumeMode::Stopped;
        current_locked.ptrace.last_stop_status = status;
        current_locked.ptrace.last_stop_kind = kind;
    }
    report_wait_event(
        &current,
        ProcessWaitEvent::Stopped {
            status,
            ptrace: true,
        },
    );
    block_current(BlockType::Stopped);
}

pub fn maybe_stop_current_on_syscall_entry() {
    let status = {
        let current = get_current_process();
        let current = current.lock();
        if current.ptrace.tracer.is_none()
            || current.ptrace.resume_mode != PtraceResumeMode::Syscall
        {
            return;
        }

        let signal = if (current.ptrace.options & PTRACE_O_TRACESYSGOOD) != 0 {
            (Signal::SIGTRAP as i32) | 0x80
        } else {
            Signal::SIGTRAP as i32
        };
        stop_status(signal)
    };

    stop_current_with_status(status, PtraceStopKind::SyscallEnter);
}

pub fn maybe_stop_current_on_syscall_exit() {
    let status = {
        let current = get_current_process();
        let current = current.lock();
        if current.ptrace.tracer.is_none()
            || current.ptrace.resume_mode != PtraceResumeMode::Syscall
        {
            return;
        }

        let signal = if (current.ptrace.options & PTRACE_O_TRACESYSGOOD) != 0 {
            (Signal::SIGTRAP as i32) | 0x80
        } else {
            Signal::SIGTRAP as i32
        };
        stop_status(signal)
    };

    stop_current_with_status(status, PtraceStopKind::SyscallExit);
}

pub fn maybe_stop_current_after_exec() {
    let status = {
        let current = get_current_process();
        let current = current.lock();
        if current.ptrace.tracer.is_none()
            || current.ptrace.resume_mode == PtraceResumeMode::Stopped
        {
            return;
        }

        if (current.ptrace.options & PTRACE_O_TRACEEXEC) != 0 {
            exec_event_status()
        } else {
            stop_status(Signal::SIGTRAP as i32)
        }
    };

    stop_current_with_status(status, PtraceStopKind::Exec);
}
