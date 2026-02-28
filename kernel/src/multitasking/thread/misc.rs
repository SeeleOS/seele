use core::sync::atomic::{AtomicU64, Ordering};

use crate::multitasking::thread::yielding::BlockType;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadID(pub u64);

impl Default for ThreadID {
    fn default() -> Self {
        static TID: AtomicU64 = AtomicU64::new(0);
        Self(TID.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum State {
    #[default]
    Ready, // ready to run (in a queue)
    Running,
    Blocked(BlockType), // stuck, waiting for something (like keyboard input)
    Zombie,             // Exited process
}
