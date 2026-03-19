use crate::{syscall, utils::SyscallResult};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PollResult {
    pub events: u32,
    pub _pad: u32,
    pub data: u64,
}

#[repr(u64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollEvent {
    CanBeRead = 0,
    CanBeWritten = 1,
    Error = 2,
    Closed = 3,
}

pub fn create_poller() -> SyscallResult {
    syscall!(CreatePoller)
}

pub fn poller_add(poller: u64, object: u64, event: PollEvent) -> SyscallResult {
    syscall!(PollerAdd, poller, object, event as u64)
}

pub fn poller_remove(poller: u64, object: u64, event: PollEvent) -> SyscallResult {
    syscall!(PollerRemove, poller, object, event as u64)
}

pub fn poller_wait(poller: u64, events: *mut PollResult, maxevents: usize) -> SyscallResult {
    syscall!(PollerWait, poller, events as u64, maxevents as u64)
}
