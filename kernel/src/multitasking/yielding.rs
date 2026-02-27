use alloc::collections::vec_deque::VecDeque;

use crate::multitasking::process::{
    ProcessRef,
    manager::Manager,
    misc::{ProcessID, State},
};

use paste::paste;
// [TODO] make the blocked process wont be pushed onto the queue.
// they should only be pushed onto the queue with the wake function

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum BlockType {
    SetTime,
    WakeRequired(WakeType),
    Futex,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum WakeType {
    Keyboard,
    IO,
}

#[derive(Clone, Debug, Default)]
pub struct BlockedQueues {
    pub keyboard: VecDeque<ProcessRef>,
    pub io: VecDeque<ProcessRef>,
}

#[macro_export]
macro_rules! register_wake_func {
    ($type: ident) => {
        paste! {
        pub fn [<wake_$type>](&mut self) {
            while let Some(pid) = self.blocked_queues.$type.pop_front() {
                self.wake(pid);
            }
        }
        }
    };
}

impl Manager {
    pub fn wake(&mut self, process: ProcessRef) {
        if matches!(process.lock().state, State::Blocked(_)) {
            process.lock().state = State::Ready;
            self.queue.push_back(process);
        }
    }

    register_wake_func!(keyboard);
    register_wake_func!(io);
}
