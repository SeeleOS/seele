use alloc::{collections::btree_map::BTreeMap, vec::Vec};
use lazy_static::lazy_static;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    filesystem::cgroupfs::remove_pid_cgroup_path,
    ipc::sysv_shm::detach_all_process_mappings,
    object::linux_anon::wake_pidfd_for_process_with_manager,
    process::{Process, ProcessExitStatus, ProcessRef, misc::ProcessID},
    signal::{Signal, send_signal_to_process},
    smp::{current_process, set_current_process},
    thread::{
        ThreadRef,
        manager::ThreadManager,
        misc::{State, ThreadID},
        with_thread_manager,
    },
};

lazy_static! {
    pub static ref MANAGER: spin::Mutex<Manager> = spin::Mutex::new(Manager::default());
}

#[derive(Debug, Default)]
pub struct Manager {
    pub processes: BTreeMap<ProcessID, ProcessRef>,
    pub zombies: Vec<ProcessRef>,
}

impl Manager {
    pub fn init(&mut self) {
        without_interrupts(|| {
            let kernel_process = Process::empty();
            self.processes
                .insert(kernel_process.lock().pid, kernel_process.clone());
            set_current_process(Some(kernel_process.clone()));

            let init = Process::init();
            self.processes.insert(init.lock().pid, init.clone());
        });
    }

    pub fn notify_process_exit_waiters(
        &mut self,
        process: ProcessRef,
        thread_manager: &mut ThreadManager,
    ) {
        let pid = process.lock().pid;
        log::debug!("notify process exit waiters {}", pid.0);
        thread_manager.wake_process_exit_waiters(pid);
        wake_pidfd_for_process_with_manager(pid.0, thread_manager);
    }

    pub fn reap_process(&mut self, process: ProcessRef) {
        let pid = process.lock().pid;
        self.processes.remove(&pid);
        remove_pid_cgroup_path(pid);
        let mut process = process.lock();
        process.close_all_fds();
        process.timers.clear();
        detach_all_process_mappings(&mut process);
        process.addrspace.clean();
    }

    pub fn load_process(&mut self, process: ProcessRef) {
        let mut process_locked = process.lock();

        process_locked.addrspace.load();
        set_current_process(Some(process.clone()));
    }
}

pub fn get_current_process() -> ProcessRef {
    current_process()
}

pub fn terminate_process(process: ProcessRef, exit_status: ProcessExitStatus) {
    let process_ref = process.clone();
    let (pid, threads, vfork_blocker, parent) = {
        let mut process = process.lock();
        process.restore_borrowed_addrspace_to_parent();
        let vfork_blocker = process.vfork_blocker.take();
        let pid = process.pid;
        (
            pid,
            process.terminate_inner(exit_status),
            vfork_blocker,
            process.parent.clone(),
        )
    };
    let reparent_target = nearest_live_subreaper(parent).or_else(init_process_ref);
    let parent_death_signals = collect_parent_death_signals_for_children(pid, &process_ref);
    let reparented_children = reparent_children(pid, &process_ref, reparent_target);

    for (child, signal) in parent_death_signals {
        send_signal_to_process(&child, signal);
    }

    let pending_child_wait_signals = reparented_children
        .iter()
        .filter_map(|child| {
            let (pid, parent, has_pending_wait) = {
                let child = child.lock();
                (
                    child.pid,
                    child.parent.clone(),
                    child.exit_status.is_some() || child.wait_event.is_some(),
                )
            };
            has_pending_wait.then_some((pid, parent?))
        })
        .collect::<Vec<_>>();

    for (_, parent) in &pending_child_wait_signals {
        send_signal_to_process(parent, Signal::SIGCHLD);
    }

    with_thread_manager(|thread_manager| {
        for (pid, _) in pending_child_wait_signals {
            thread_manager.wake_process_exit_waiters(pid);
        }
        if let Some(thread_id) = vfork_blocker {
            thread_manager.wake_thread_by_id(thread_id);
        }
        for thread in threads {
            thread_manager.mark_thread_exited(thread);
        }
        thread_manager.cleanup_exited_threads();
    });
}

fn collect_parent_death_signals_for_children(
    parent_pid: ProcessID,
    parent_process: &ProcessRef,
) -> Vec<(ProcessRef, Signal)> {
    MANAGER
        .lock()
        .processes
        .values()
        .filter_map(|candidate| {
            if alloc::sync::Arc::ptr_eq(candidate, parent_process) {
                return None;
            }
            let child = candidate.clone();
            let child_lock = child.lock();
            let parent = child_lock.parent.clone()?;
            if child_lock.pid == parent_pid || !alloc::sync::Arc::ptr_eq(&parent, parent_process) {
                return None;
            }
            Some((child.clone(), child_lock.parent_death_signal?))
        })
        .collect()
}

fn init_process_ref() -> Option<ProcessRef> {
    MANAGER.lock().processes.values().find_map(|process| {
        let process = process.clone();
        let is_init = process.lock().pid.0 == 1;
        is_init.then_some(process)
    })
}

fn nearest_live_subreaper(mut current: Option<ProcessRef>) -> Option<ProcessRef> {
    while let Some(candidate) = current {
        let next = {
            let candidate_lock = candidate.lock();
            if candidate_lock.exit_status.is_none() && candidate_lock.child_subreaper {
                return Some(candidate.clone());
            }
            candidate_lock.parent.clone()
        };
        current = next;
    }

    None
}

fn reparent_children(
    parent_pid: ProcessID,
    parent_process: &ProcessRef,
    reparent_target: Option<ProcessRef>,
) -> Vec<ProcessRef> {
    let manager = MANAGER.lock();
    let mut reparented = Vec::new();

    for candidate in manager.processes.values() {
        if alloc::sync::Arc::ptr_eq(candidate, parent_process) {
            continue;
        }

        let child = candidate.clone();
        let should_reparent = {
            let child_lock = child.lock();
            let parent = child_lock.parent.clone();
            child_lock.pid != parent_pid
                && parent
                    .as_ref()
                    .is_some_and(|parent| alloc::sync::Arc::ptr_eq(parent, parent_process))
        };
        if !should_reparent {
            continue;
        }

        child.lock().parent = reparent_target.clone();
        reparented.push(child);
    }

    reparented
}

pub fn exit_current_thread(exit_status: ProcessExitStatus) {
    let current = crate::thread::get_current_thread();
    let process = current.lock().parent.clone();
    let live_threads = {
        let process = process.lock();
        process
            .threads
            .iter()
            .filter_map(|thread| thread.upgrade())
            .filter(|thread| !matches!(thread.lock().state, State::Zombie))
            .count()
    };

    if live_threads <= 1 {
        terminate_process(process, exit_status);
        return;
    }

    with_thread_manager(|thread_manager| {
        thread_manager.mark_thread_exited(current);
        thread_manager.cleanup_exited_threads();
    });
}

pub fn wake_vfork_blocker(thread_id: ThreadID) {
    with_thread_manager(|thread_manager| thread_manager.wake_thread_by_id(thread_id));
}

impl Process {
    #[must_use]
    pub fn terminate_inner(&mut self, exit_status: ProcessExitStatus) -> Vec<ThreadRef> {
        if self.exit_status.is_none() {
            self.exit_status = Some(exit_status);
            remove_pid_cgroup_path(self.pid);
        }

        self.close_all_fds();

        self.threads
            .iter()
            .filter_map(|thread| thread.upgrade())
            .collect()
    }
}
