use alloc::{collections::vec_deque::VecDeque, sync::Arc, vec::Vec};
use seele_sys::SyscallResult;

use crate::{
    misc::time::Time,
    object::{
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
    },
    polling::event::PollableEvent,
    process::{manager::get_current_process, misc::ProcessID},
    task::{TASK_SPAWNER, task::Task},
    thread::{
        THREAD_MANAGER, ThreadRef, future::ThreadFuture, manager::ThreadManager, misc::State,
        scheduling::return_to_executor_from_current,
    },
};

use paste::paste;
// [TODO] make the blocked process wont be pushed onto the queue.
// they should only be pushed onto the queue with the wake function

#[derive(Clone, Debug)]
pub enum BlockType {
    SetTime(Time),
    WakeRequired {
        wake_type: WakeType,
        deadline: Option<Time>,
    },
    Futex,
    Stopped,
}

impl BlockType {
    pub fn is_timed_out(&self) -> bool {
        match self {
            BlockType::SetTime(time) => *time <= Time::since_boot(),
            BlockType::WakeRequired {
                deadline: Some(deadline),
                ..
            } => *deadline <= Time::since_boot(),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum WakeType {
    Pty,
    Mouse,
    Keyboard,
    // Waiting for a process to exit
    ProcsesExit(ProcessID),
    IO,
    // Blocked by the polling system
    // the first argument points to the poller.
    Poller(ObjectRef),
}

#[derive(Clone, Debug, Default)]
pub struct BlockedQueues {
    pub keyboard: VecDeque<ThreadRef>,
    pub pty: VecDeque<ThreadRef>,
    pub io: VecDeque<ThreadRef>,
    pub process_exit: VecDeque<ThreadRef>,
    pub mouse: VecDeque<ThreadRef>,
    pub poller: VecDeque<ThreadRef>,

    pub any: VecDeque<ThreadRef>,
}

impl BlockedQueues {
    pub fn get_appropriate_queue(&mut self, wake_type: WakeType) -> &mut VecDeque<ThreadRef> {
        match wake_type {
            WakeType::Pty => &mut self.pty,
            WakeType::Keyboard => &mut self.keyboard,
            WakeType::Mouse => &mut self.mouse,
            WakeType::ProcsesExit(_) => &mut self.process_exit,
            WakeType::Poller(_) => &mut self.poller,
            WakeType::IO => &mut self.io,
        }
    }

    pub fn push(&mut self, thread_ref: ThreadRef, block_type: BlockType) {
        self.any.push_back(thread_ref.clone());

        if let BlockType::WakeRequired { wake_type, .. } = block_type {
            self.get_appropriate_queue(wake_type).push_back(thread_ref)
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
    // Process all the blocked thread for timed out ones and wake them
    pub fn process_timed_out_threads(&mut self) {
        let mut to_wake = Vec::new();

        for thread in &self.blocked_queues.any {
            if let State::Blocked(block_type) = &thread.lock().state
                && block_type.is_timed_out()
            {
                to_wake.push(thread.clone());
            }
        }

        for thread in to_wake {
            self.wake(thread);
        }
    }

    fn remove_from_blocked_queues(&mut self, thread: &ThreadRef) {
        self.blocked_queues
            .keyboard
            .retain(|t| !Arc::ptr_eq(t, thread));
        self.blocked_queues
            .mouse
            .retain(|t| !Arc::ptr_eq(t, thread));
        self.blocked_queues.pty.retain(|t| !Arc::ptr_eq(t, thread));
        self.blocked_queues.io.retain(|t| !Arc::ptr_eq(t, thread));
        self.blocked_queues
            .process_exit
            .retain(|t| !Arc::ptr_eq(t, thread));
        self.blocked_queues
            .poller
            .retain(|t| !Arc::ptr_eq(t, thread));
        self.blocked_queues.any.retain(|t| !Arc::ptr_eq(t, thread));
    }

    fn block(&mut self, thread_ref: ThreadRef, block_type: BlockType) {
        log::debug!("thread block: {:?}", block_type);
        let mut thread = thread_ref.lock();

        thread.state = State::Blocked(block_type.clone());

        self.blocked_queues.push(thread_ref.clone(), block_type);
    }

    pub fn wake(&mut self, thread: ThreadRef) {
        log::debug!("thread wake");
        self.remove_from_blocked_queues(&thread);
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

    pub fn wake_process_exit_waiters(&mut self, pid: ProcessID) {
        log::debug!("thread wake_process_exit_waiters: {}", pid.0);
        let mut to_wake = Vec::new();

        self.blocked_queues.process_exit.retain(|f| {
            if let State::Blocked(BlockType::WakeRequired {
                wake_type: WakeType::ProcsesExit(target_pid),
                ..
            }) = f.lock().state
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

    pub fn wake_poller(&mut self, target_object: ObjectRef, event: PollableEvent) {
        let mut to_wake = Vec::new();

        self.blocked_queues.poller.retain(|f| {
            if let State::Blocked(BlockType::WakeRequired {
                wake_type: WakeType::Poller(poller),
                ..
            }) = &f.lock().state
            {
                let should_wake = poller
                    .clone()
                    .as_poller()
                    .unwrap()
                    .push_woken_event(target_object.clone(), event);
                if should_wake {
                    to_wake.push(f.clone());
                    false
                } else {
                    true
                }
            } else {
                true
            }
        });

        to_wake.iter().for_each(|f| self.wake(f.clone()));
    }

    register_wake_func!(pty);
    register_wake_func!(keyboard);
    register_wake_func!(mouse);
    register_wake_func!(io);
}

pub fn block(thread_ref: ThreadRef, block_type: BlockType) {
    {
        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();

        thread_manager.block(thread_ref, block_type);
    }

    return_to_executor_from_current();
}

pub fn block_current(block_type: BlockType) {
    let current = THREAD_MANAGER
        .get()
        .unwrap()
        .lock()
        .current
        .clone()
        .unwrap();
    block(current, block_type);
}

// Avoid sleeping forever in interruptible waits by re-checking for pending
// signals before and after blocking
#[must_use]
pub fn block_current_with_sig_check(block_type: BlockType) -> ObjectResult<()> {
    if !get_current_process().lock().pending_signals.is_empty() {
        return Err(ObjectError::Interrupted);
    }
    block_current(block_type);
    if !get_current_process().lock().pending_signals.is_empty() {
        return Err(ObjectError::Interrupted);
    }
    Ok(())
}
