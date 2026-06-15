use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::utils::Mut,
    object::{
        FileFlags, Object,
        misc::{ObjectRef, ObjectResult},
        traits::Statable,
    },
    polling::{event::PollableEvent, object::Pollable},
    process::{manager::MANAGER, misc::ProcessID},
    thread::{manager::ThreadManager, yielding::wake_pollers_for_object},
};

use super::registry::WatcherRegistry;

lazy_static::lazy_static! {
    static ref PIDFD_REGISTRY: Mut<WatcherRegistry<PidFdObject>> = Mut::new(WatcherRegistry::default());
}

#[derive(Debug)]
pub struct PidFdObject {
    flags: Mut<FileFlags>,
    pid: u64,
    alive: AtomicBool,
    process: Mut<Option<Weak<crate::memory::utils::Mut<crate::process::Process>>>>,
    self_ref: Mut<Option<Weak<PidFdObject>>>,
}

impl PidFdObject {
    pub fn new(pid: u64) -> Arc<Self> {
        let process = MANAGER.lock().processes.get(&ProcessID(pid)).cloned();
        let alive = process
            .as_ref()
            .is_some_and(|process| !process.lock().have_exited());
        let pidfd = Arc::new(Self {
            flags: Mut::new(FileFlags::empty()),
            pid,
            alive: AtomicBool::new(alive),
            process: Mut::new(process.as_ref().map(Arc::downgrade)),
            self_ref: Mut::new(None),
        });
        *pidfd.self_ref.lock() = Some(Arc::downgrade(&pidfd));
        register_pidfd(pid, &pidfd);
        pidfd
    }

    pub fn pid(&self) -> u64 {
        self.pid
    }

    fn self_object(&self) -> Option<ObjectRef> {
        self.self_ref
            .lock()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|object| object as ObjectRef)
    }

    fn is_alive(&self) -> bool {
        if !self.alive.load(Ordering::Acquire) {
            return false;
        }

        let Some(process) = self.process.lock().as_ref().and_then(Weak::upgrade) else {
            self.alive.store(false, Ordering::Release);
            return false;
        };

        let alive = !process.lock().have_exited();
        if !alive {
            self.alive.store(false, Ordering::Release);
        }
        alive
    }

    fn mark_exited(&self) {
        self.alive.store(false, Ordering::Release);
    }

    fn wake_waiters_with_manager(&self, manager: &mut ThreadManager) {
        manager.wake_io();
    }
}

fn register_pidfd(pid: u64, pidfd: &Arc<PidFdObject>) {
    PIDFD_REGISTRY.lock().register(pid, pidfd);
}

fn pidfds_for_process(pid: u64) -> Vec<Arc<PidFdObject>> {
    PIDFD_REGISTRY.lock().live_watchers(pid)
}

pub fn wake_pidfd_for_process_with_manager(pid: u64, manager: &mut ThreadManager) {
    for pidfd in pidfds_for_process(pid) {
        pidfd.mark_exited();
        pidfd.wake_waiters_with_manager(manager);
    }
}

pub fn wake_pidfd_for_process(pid: u64) {
    let watchers = pidfds_for_process(pid);
    if watchers.is_empty() {
        return;
    }

    let mut poller_objects = Vec::new();
    crate::thread::with_thread_manager(|manager| {
        for pidfd in &watchers {
            pidfd.mark_exited();
            pidfd.wake_waiters_with_manager(manager);
            if let Some(object) = pidfd.self_object() {
                poller_objects.push(object);
            }
        }
    });
    for object in poller_objects {
        wake_pollers_for_object(object, PollableEvent::CanBeRead);
    }
}

impl Object for PidFdObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("pidfd", PidFdObject);
}

impl Pollable for PidFdObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead) && !self.is_alive()
    }
}

impl Statable for PidFdObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}
