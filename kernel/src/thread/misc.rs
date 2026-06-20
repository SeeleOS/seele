use crate::{
    define_with_accessor,
    process::misc::next_linux_task_id,
    thread::{get_current_thread, snapshot::ThreadSnapshot, thread::Thread, yielding::BlockType},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockedSyscall {
    Accept,
    Accept4,
    Wait4,
    Waitid,
    Poll,
    Ppoll,
    EpollWait,
    Pselect6,
    Nanosleep,
    ClockNanosleep,
    Pause,
    Futex,
}

impl BlockedSyscall {
    pub fn from_syscall_number(number: usize) -> Option<Self> {
        use crate::systemcall::numbers::SyscallNumber;

        match SyscallNumber::from_number(number)? {
            SyscallNumber::Accept => Some(Self::Accept),
            SyscallNumber::Accept4 => Some(Self::Accept4),
            SyscallNumber::Wait4 => Some(Self::Wait4),
            SyscallNumber::Waitid => Some(Self::Waitid),
            SyscallNumber::Poll => Some(Self::Poll),
            SyscallNumber::Ppoll => Some(Self::Ppoll),
            SyscallNumber::EpollWait | SyscallNumber::EpollPwait | SyscallNumber::EpollPwait2 => {
                Some(Self::EpollWait)
            }
            SyscallNumber::Pselect6 => Some(Self::Pselect6),
            SyscallNumber::Nanosleep => Some(Self::Nanosleep),
            SyscallNumber::ClockNanosleep => Some(Self::ClockNanosleep),
            SyscallNumber::Pause => Some(Self::Pause),
            SyscallNumber::Futex => Some(Self::Futex),
            _ => None,
        }
    }

    pub fn syscall_number(self) -> usize {
        use crate::systemcall::numbers::SyscallNumber;

        match self {
            Self::Accept => SyscallNumber::Accept as usize,
            Self::Accept4 => SyscallNumber::Accept4 as usize,
            Self::Wait4 => SyscallNumber::Wait4 as usize,
            Self::Waitid => SyscallNumber::Waitid as usize,
            Self::Poll => SyscallNumber::Poll as usize,
            Self::Ppoll => SyscallNumber::Ppoll as usize,
            Self::EpollWait => SyscallNumber::EpollWait as usize,
            Self::Pselect6 => SyscallNumber::Pselect6 as usize,
            Self::Nanosleep => SyscallNumber::Nanosleep as usize,
            Self::ClockNanosleep => SyscallNumber::ClockNanosleep as usize,
            Self::Pause => SyscallNumber::Pause as usize,
            Self::Futex => SyscallNumber::Futex as usize,
        }
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadID(pub u64);

impl ThreadID {
    pub fn new() -> Self {
        Self(next_linux_task_id())
    }
}

#[derive(Default, Clone, Debug)]
pub enum State {
    #[default]
    Ready, // ready to run (in a queue)
    Running,
    Blocking(BlockType), // preparing to sleep but still running on its CPU
    Woken,               // wakeup raced with Blocking before the CPU switched out
    Blocked(BlockType),  // stuck, waiting for something (like keyboard input)
    Exiting,             // running on another CPU and must stop at the next scheduler return
    Zombie,              // Exited process
}

/// Selects which execution context of the thread should be resumed next.
///
/// This is separate from [`State`]:
/// - [`State`] describes scheduler state such as ready/running/blocked.
/// - `SnapshotState` describes which saved CPU context is currently active
///   within the thread itself.
///
/// Keeping this as an enum leaves room for extra contexts later, such as
/// signal handlers or other user-mode upcalls.
#[derive(Default, Clone, Copy, Debug)]
pub enum SnapshotState {
    #[default]
    Normal,
    SignalHandler,
}

impl Thread {
    pub fn get_appropriate_snapshot(&mut self) -> &mut ThreadSnapshot {
        match self.snapshot_state {
            SnapshotState::Normal => &mut self.snapshot,
            SnapshotState::SignalHandler => &mut self.sig_handler_snapshot,
        }
    }
}

define_with_accessor!("current_thread", Thread, get_current_thread);
