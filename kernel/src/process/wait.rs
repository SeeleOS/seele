use crate::{
    process::{Process, ProcessRef},
    signal::{Signal, send_signal_to_process},
    thread::THREAD_MANAGER,
};

#[derive(Clone, Copy, Debug)]
pub enum ProcessWaitEvent {
    Stopped { status: i32, ptrace: bool },
}

impl ProcessWaitEvent {
    pub fn wait_status(self) -> i32 {
        match self {
            Self::Stopped { status, .. } => status,
        }
    }

    pub fn is_ptrace(self) -> bool {
        match self {
            Self::Stopped { ptrace, .. } => ptrace,
        }
    }
}

pub fn report_wait_event(process: &ProcessRef, event: ProcessWaitEvent) {
    let (pid, parent) = {
        let mut process = process.lock();
        process.wait_event = Some(event);
        (process.pid, process.parent.clone())
    };

    if let Some(parent) = parent {
        send_signal_to_process(&parent, Signal::SIGCHLD);
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .wake_process_exit_waiters(pid);
    }
}

pub fn take_wait_event(process: &mut Process, preserve: bool) -> Option<ProcessWaitEvent> {
    if preserve {
        process.wait_event
    } else {
        process.wait_event.take()
    }
}
