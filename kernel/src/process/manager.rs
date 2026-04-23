use alloc::{collections::btree_map::BTreeMap, vec::Vec};
use lazy_static::lazy_static;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{
    filesystem::cgroupfs::remove_pid_cgroup_path,
    misc::systemd_perf,
    object::linux_anon::wake_pidfd_for_process_with_manager,
    process::{Process, ProcessRef, misc::ProcessID},
    smp::{current_process, set_current_process},
    thread::{THREAD_MANAGER, ThreadRef, manager::ThreadManager},
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
        process.objects.clear();
        process.object_flags.clear();
        process.timers.clear();
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

pub fn terminate_process(process: ProcessRef, exit_code: u64) {
    let threads = {
        let mut process = process.lock();
        systemd_perf::log_and_clear_process_summary(&process, exit_code);
        process.terminate_inner(exit_code)
    };

    let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
    for thread in threads {
        thread_manager.mark_thread_exited(thread);
    }
    thread_manager.cleanup_exited_threads();
}

impl Process {
    #[must_use]
    pub fn terminate_inner(&mut self, exit_code: u64) -> Vec<ThreadRef> {
        if self.exit_code.is_none() {
            self.exit_code = Some(exit_code);
            remove_pid_cgroup_path(self.pid);
        }

        self.objects.clear();
        self.object_flags.clear();

        self.threads
            .iter()
            .filter_map(|thread| thread.upgrade())
            .collect()
    }
}
