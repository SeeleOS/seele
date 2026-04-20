use crate::{
    define_with_accessor,
    process::misc::next_linux_task_id,
    thread::{get_current_thread, snapshot::ThreadSnapshot, thread::Thread, yielding::BlockType},
};

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
    Blocked(BlockType), // stuck, waiting for something (like keyboard input)
    Zombie,             // Exited process
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
