use alloc::collections::vec_deque::VecDeque;

use crate::multitasking::{
    kernel_task::{TASK_SPAWNER, task::Task},
    process::misc::ProcessID,
    thread::{ThreadRef, future::ThreadFuture, manager::ThreadManager, misc::State},
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
    // Waiting for a process to exit
    ProcsesExit(ProcessID),
    IO,
}

#[derive(Clone, Debug, Default)]
pub struct BlockedQueues {
    pub keyboard: VecDeque<ThreadRef>,
    pub io: VecDeque<ThreadRef>,
    pub process_exit: VecDeque<ThreadRef>,
}

#[macro_export]
macro_rules! register_wake_func {
    ($type: ident) => {
        paste! {
        pub fn [<wake_$type>](&mut self) {
            while let Some(thread) = self.blocked_queues.$type.pop_front() {
                self.wake(thread);
            }
        }
        }
    };
}

impl ThreadManager {
    pub fn wake(&mut self, thread: ThreadRef) {
        let mut locked_thread = thread.lock();
        if matches!(locked_thread.state, State::Blocked(_)) {
            locked_thread.state = State::Ready;
            TASK_SPAWNER
                .get()
                .unwrap()
                .lock()
                .spawn(Task::new(ThreadFuture(thread.clone())));
        }
    }

    register_wake_func!(keyboard);
    register_wake_func!(io);
}
