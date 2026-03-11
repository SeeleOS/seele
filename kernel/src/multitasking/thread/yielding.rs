use core::ptr::with_exposed_provenance;

use alloc::{collections::vec_deque::VecDeque, vec::Vec};

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

impl BlockedQueues {
    pub fn push(&mut self, thread_ref: ThreadRef, block_type: BlockType) {
        match block_type {
            BlockType::WakeRequired(wake_type) => match wake_type {
                WakeType::ProcsesExit(_) => self.process_exit.push_back(thread_ref),
                WakeType::Keyboard => self.keyboard.push_back(thread_ref),
                WakeType::IO => self.io.push_back(thread_ref),
            },
            _ => unimplemented!(),
        }
    }
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
    pub fn block(&mut self, thread_ref: ThreadRef, block_type: BlockType) {
        let mut thread = thread_ref.lock();

        thread.state = State::Blocked(block_type);

        self.blocked_queues.push(thread_ref.clone(), block_type);
    }

    pub fn block_current(&mut self, block_type: BlockType) {
        let current = self.current.clone().unwrap();
        self.block(current, block_type);
    }

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

    pub fn wake_process_exit(&mut self, pid: ProcessID) {
        let mut to_wake = Vec::new();

        self.blocked_queues.process_exit.retain(|f| {
            if let State::Blocked(BlockType::WakeRequired(WakeType::ProcsesExit(target_pid))) =
                f.lock().state
                && target_pid.0 == pid.0
            {
                to_wake.push(f.clone());
                false
            } else {
                true
            }
        });

        for thread in to_wake {
            self.wake(thread);
        }
    }

    register_wake_func!(keyboard);
    register_wake_func!(io);
}
