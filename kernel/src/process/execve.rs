use alloc::{string::String, vec::Vec};
use x86_64::VirtAddr;

use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS},
    misc::time::with_profiling,
    process::{Process, manager::MANAGER, new::setup_process},
    signal::{Signals, misc::default_signal_action_vec},
    thread::{
        THREAD_MANAGER, misc::SnapshotState, snapshot::ThreadSnapshot, stack::allocate_kernel_stack,
    },
    tss::TSS,
};

impl Process {
    fn execve(
        &mut self,
        path: Path,
        args: Vec<String>,
        env: Vec<String>,
    ) -> Result<*mut ThreadSnapshot, FSError> {
        if VirtualFS.lock().resolve(path.clone()).is_err() {
            return Err(FSError::NotFound);
        }
        let path_string = path.clone().as_string();
        // TODO: kill all the other threads when execveing
        log::trace!("execve: start {}", path.clone().as_string());
        with_profiling(
            || self.addrspace.clean(),
            alloc::format!(
                "execve clean addrspace pid={} path={}",
                self.pid.0,
                path_string
            )
            .as_str(),
        );

        log::trace!("execve: locking thread manager");
        let thread_manager = THREAD_MANAGER.get().unwrap().lock();
        log::trace!("execve: thread manager locked");

        let thread = thread_manager.current.clone().unwrap();

        log::trace!("execve: kill all except current");
        //thread_manager.kill_all_except(thread.clone());
        log::trace!("execve: kill all done");

        // Reallocates the kernel stack top (just in case)
        self.kernel_stack_top = with_profiling(
            || allocate_kernel_stack(16).finish(),
            alloc::format!(
                "execve allocate kernel stack pid={} path={}",
                self.pid.0,
                path_string
            )
            .as_str(),
        );

        log::trace!("execve: locking current thread");
        let mut thread_locked = thread.lock();
        log::trace!("execve: current thread locked");

        thread_locked.snapshot = with_profiling(
            || setup_process(path, args, env, &mut self.addrspace, &mut self.objects),
            alloc::format!(
                "execve setup_process pid={} path={}",
                self.pid.0,
                path_string
            )
            .as_str(),
        )
        .unwrap();
        thread_locked.kernel_stack_top = self.kernel_stack_top.as_u64();
        thread_locked.snapshot_state = SnapshotState::Normal;
        thread_locked.sig_handler_snapshot = ThreadSnapshot::default();
        thread_locked.saved_blocked_signals.clear();
        thread_locked.blocked_signals = Signals::default();
        self.pending_signals = Signals::default();
        self.signal_actions = default_signal_action_vec();
        self.program_break = 0;

        with_profiling(
            || self.addrspace.load(),
            alloc::format!(
                "execve addrspace.load pid={} path={}",
                self.pid.0,
                path_string
            )
            .as_str(),
        );
        unsafe {
            TSS.privilege_stack_table[0] = VirtAddr::new(thread_locked.kernel_stack_top);
        }

        Ok(&mut thread_locked.snapshot as *mut ThreadSnapshot)
    }
}

pub fn execve(path: Path, args: Vec<String>, env: Vec<String>) -> Result<(), FSError> {
    let snapshot = {
        log::debug!("execve: locking process manager");
        let manager = MANAGER.lock();
        log::debug!("execve: process manager locked");
        let current = manager.current.clone().unwrap();
        with_profiling(
            || current.lock().execve(path, args, env),
            "process::execve total",
        )?
    };

    unsafe { (*snapshot).switch_from(None, None) };

    unreachable!();
}
