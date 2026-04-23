use alloc::{
    collections::{BTreeMap, vec_deque::VecDeque},
    format,
    string::String,
    sync::Arc,
    vec::Vec,
};

use crate::{
    misc::{systemd_perf, time::Time},
    object::{
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
    },
    polling::event::PollableEvent,
    process::{manager::get_current_process, misc::ProcessID},
    systemcall::implementations::remove_futex_waiter,
    thread::{
        THREAD_MANAGER, ThreadRef, manager::ThreadManager, misc::State, misc::ThreadID,
        scheduling::return_to_scheduler_from_current,
    },
};

use paste::paste;
// [TODO] make the blocked process wont be pushed onto the queue.
// they should only be pushed onto the queue with the wake function

fn pollable_object_kind(object: &ObjectRef) -> &'static str {
    if object.clone().as_netlink_socket().is_ok() {
        "netlink"
    } else if object.clone().as_signalfd().is_ok() {
        "signalfd"
    } else if object.clone().as_timerfd().is_ok() {
        "timerfd"
    } else if object.clone().as_eventfd().is_ok() {
        "eventfd"
    } else if object.clone().as_inotify().is_ok() {
        "inotify"
    } else if object.clone().as_unix_socket().is_ok() {
        "unix"
    } else if object.clone().as_pidfd().is_ok() {
        "pidfd"
    } else if object.clone().as_poller().is_ok() {
        "poller"
    } else {
        "other"
    }
}

fn poll_event_name(event: PollableEvent) -> &'static str {
    match event {
        PollableEvent::CanBeRead => "read",
        PollableEvent::CanBeWritten => "write",
        PollableEvent::Error => "error",
        PollableEvent::Closed => "closed",
        PollableEvent::Other(_) => "other",
    }
}

fn log_current_poller_details(poller: &ObjectRef) {
    let Ok(poller) = poller.clone().as_poller() else {
        return;
    };

    let mut counts = BTreeMap::<String, usize>::new();
    for entry in poller.entries.lock().iter() {
        let key = format!(
            "{}:{}",
            pollable_object_kind(&entry.object),
            poll_event_name(entry.event)
        );
        *counts.entry(key).or_default() += 1;
    }

    let mut parts = Vec::new();
    for (key, count) in counts {
        parts.push(format!("{key}={count}"));
    }

    if !parts.is_empty() {
        let detail = format!("poller[{}]", parts.join(","));
        systemd_perf::log_current_block(&detail);
    }
}

#[derive(Clone, Debug)]
pub enum BlockType {
    SetTime(Time),
    WakeRequired {
        wake_type: WakeType,
        deadline: Option<Time>,
    },
    Futex {
        deadline: Option<Time>,
    },
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
            BlockType::Futex {
                deadline: Some(deadline),
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
    // Waiting for any child-process state change. The waiter re-checks its
    // exact waitpid/wait4 filter after wakeup.
    ProcsesExit,
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
    pub timed: BTreeMap<(Time, ThreadID), ThreadRef>,
}

impl BlockedQueues {
    pub fn get_appropriate_queue(&mut self, wake_type: WakeType) -> &mut VecDeque<ThreadRef> {
        match wake_type {
            WakeType::Pty => &mut self.pty,
            WakeType::Keyboard => &mut self.keyboard,
            WakeType::Mouse => &mut self.mouse,
            WakeType::ProcsesExit => &mut self.process_exit,
            WakeType::Poller(_) => &mut self.poller,
            WakeType::IO => &mut self.io,
        }
    }

    pub fn push(&mut self, thread_ref: ThreadRef, thread_id: ThreadID, block_type: BlockType) {
        let deadline = block_deadline(&block_type);

        if let Some(deadline) = deadline {
            self.timed.insert((deadline, thread_id), thread_ref.clone());
        }

        if let BlockType::WakeRequired { wake_type, .. } = block_type {
            self.get_appropriate_queue(wake_type).push_back(thread_ref)
        }
    }
}

fn block_deadline(block_type: &BlockType) -> Option<Time> {
    match block_type {
        BlockType::SetTime(time) => Some(*time),
        BlockType::WakeRequired {
            deadline: Some(deadline),
            ..
        } => Some(*deadline),
        BlockType::Futex {
            deadline: Some(deadline),
        } => Some(*deadline),
        BlockType::WakeRequired { deadline: None, .. }
        | BlockType::Futex { deadline: None }
        | BlockType::Stopped => None,
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
        let now = Time::since_boot();

        while let Some((&(deadline, _), _)) = self.blocked_queues.timed.first_key_value() {
            if deadline > now {
                break;
            }

            let Some((_, thread)) = self.blocked_queues.timed.pop_first() else {
                break;
            };

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

    pub(crate) fn remove_from_blocked_queues(&mut self, thread: &ThreadRef) {
        remove_futex_waiter(thread);
        let timed_key = {
            let thread = thread.lock();
            match &thread.state {
                State::Blocked(block_type) => {
                    block_deadline(block_type).map(|deadline| (deadline, thread.id))
                }
                State::Ready | State::Running | State::Zombie => None,
            }
        };
        if let Some(key) = timed_key {
            self.blocked_queues.timed.remove(&key);
        }
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
    }

    fn block(&mut self, thread_ref: ThreadRef, block_type: BlockType) {
        log::debug!("thread block: {:?}", block_type);
        let mut thread = thread_ref.lock();
        let thread_id = thread.id;

        thread.state = State::Blocked(block_type.clone());

        self.blocked_queues
            .push(thread_ref.clone(), thread_id, block_type);
    }

    pub fn wake(&mut self, thread: ThreadRef) {
        log::debug!("thread wake");
        self.remove_from_blocked_queues(&thread);
        let mut locked_thread = thread.lock();
        if matches!(locked_thread.state, State::Blocked(_)) {
            locked_thread.state = State::Ready;
            drop(locked_thread);
            self.push_ready(thread);
        }
    }

    pub fn wake_process_exit_waiters(&mut self, pid: ProcessID) {
        log::debug!("thread wake_process_exit_waiters: {}", pid.0);
        let mut to_wake = Vec::new();

        self.blocked_queues.process_exit.retain(|f| {
            if let State::Blocked(BlockType::WakeRequired {
                wake_type: WakeType::ProcsesExit,
                ..
            }) = f.lock().state
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
        let affected_pollers = crate::polling::notify_pollers(target_object, event);

        self.blocked_queues.poller.retain(|f| {
            if let State::Blocked(BlockType::WakeRequired {
                wake_type: WakeType::Poller(poller),
                ..
            }) = &f.lock().state
            {
                let should_wake = affected_pollers
                    .iter()
                    .any(|affected| Arc::ptr_eq(affected, poller));
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

    pub fn wake_ready_pollers(&mut self) {
        let mut to_wake = Vec::new();

        for thread in &self.blocked_queues.poller {
            let should_wake = if let State::Blocked(BlockType::WakeRequired {
                wake_type: WakeType::Poller(poller),
                ..
            }) = &thread.lock().state
            {
                if let Ok(poller) = poller.clone().as_poller() {
                    poller.has_woken_events() || poller.push_already_ready_events()
                } else {
                    false
                }
            } else {
                false
            };

            if should_wake {
                to_wake.push(thread.clone());
            }
        }

        for thread in to_wake {
            self.wake(thread);
        }
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

    return_to_scheduler_from_current();
}

fn current_thread_ref() -> ThreadRef {
    crate::thread::get_current_thread()
}

pub fn prepare_block_current(block_type: BlockType) -> ThreadRef {
    let block_kind = match &block_type {
        BlockType::SetTime(_) => "set_time",
        BlockType::WakeRequired {
            wake_type: WakeType::IO,
            ..
        } => "io",
        BlockType::WakeRequired {
            wake_type: WakeType::Poller(_),
            ..
        } => "poller",
        BlockType::WakeRequired {
            wake_type: WakeType::Pty,
            ..
        } => "pty",
        BlockType::WakeRequired {
            wake_type: WakeType::Mouse,
            ..
        } => "mouse",
        BlockType::WakeRequired {
            wake_type: WakeType::Keyboard,
            ..
        } => "keyboard",
        BlockType::WakeRequired {
            wake_type: WakeType::ProcsesExit,
            ..
        } => "process_exit",
        BlockType::Futex { .. } => "futex_wait",
        BlockType::Stopped => "stopped",
    };
    systemd_perf::log_current_block(block_kind);
    if let BlockType::WakeRequired {
        wake_type: WakeType::Poller(poller),
        ..
    } = &block_type
    {
        log_current_poller_details(poller);
    }

    let current = current_thread_ref();

    {
        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
        thread_manager.block(current.clone(), block_type);
    }

    current
}

pub fn cancel_block(thread_ref: &ThreadRef) {
    let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
    thread_manager.remove_from_blocked_queues(thread_ref);

    let mut thread = thread_ref.lock();
    if matches!(thread.state, State::Blocked(_)) {
        thread.state = State::Running;
    }
}

pub fn finish_block_current() {
    let current = current_thread_ref();
    {
        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
        let mut thread = current.lock();
        match thread.state {
            State::Blocked(_) => {}
            State::Ready => {
                thread_manager
                    .ready_queue
                    .retain(|queued| !Arc::ptr_eq(queued, &current));
                thread.state = State::Running;
                return;
            }
            _ => return,
        }
    }

    return_to_scheduler_from_current();
}

pub fn block_current(block_type: BlockType) {
    prepare_block_current(block_type);
    finish_block_current();
}

// Avoid sleeping forever in interruptible waits by re-checking for pending
// signals before and after blocking
pub fn block_current_with_sig_check(block_type: BlockType) -> ObjectResult<()> {
    if !get_current_process().lock().pending_signals.is_empty() {
        return Err(ObjectError::Interrupted);
    }
    prepare_block_current(block_type);
    finish_block_current();
    if !get_current_process().lock().pending_signals.is_empty() {
        return Err(ObjectError::Interrupted);
    }
    Ok(())
}
