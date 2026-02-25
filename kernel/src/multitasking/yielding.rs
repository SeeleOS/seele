use alloc::collections::vec_deque::VecDeque;

use crate::multitasking::process::{
    manager::Manager,
    process::{ProcessID, State},
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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct BlockedQueues {
    pub keyboard: VecDeque<ProcessID>,
    pub io: VecDeque<ProcessID>,
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
    pub fn wake(&mut self, process: ProcessID) {
        if let Some(process) = self.processes.get_mut(&process) {
            if matches!(process.state, State::Blocked(_)) {
                process.state = State::Ready;
                self.queue.push_back(process.pid);
            }
        }
    }

    register_wake_func!(keyboard);
    register_wake_func!(io);
}
