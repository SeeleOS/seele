use core::mem;

use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS},
    ipc::sysv_shm::detach_all_process_mappings,
    memory::addrspace::AddrSpace,
    misc::time::with_profiling,
    process::{
        Process,
        manager::{MANAGER, wake_vfork_blocker},
        new::setup_process,
        object::close_cloexec_fd_entries,
        ptrace::maybe_stop_current_after_exec,
    },
    signal::{
        Signals,
        action::{SignalAction, SignalHandlingType},
        misc::default_signal_action_vec,
    },
    smp::{current_process, current_thread, set_current_kernel_stack},
    thread::{
        misc::{SnapshotState, ThreadID},
        snapshot::ThreadSnapshot,
        stack::allocate_kernel_stack,
    },
};
use alloc::{string::String, vec, vec::Vec};

fn execve_signal_actions(old_actions: &[SignalAction]) -> Vec<SignalAction> {
    let defaults = default_signal_action_vec();
    old_actions
        .iter()
        .zip(defaults)
        .map(|(old, default)| match old.handling_type {
            SignalHandlingType::Ignore => old.clone(),
            SignalHandlingType::Default => default,
            SignalHandlingType::Function1(_) | SignalHandlingType::Function2(_) => default,
        })
        .collect()
}

impl Process {
    fn execve(
        &mut self,
        path: Path,
        args: Vec<String>,
        env: Vec<String>,
    ) -> Result<(*mut ThreadSnapshot, Option<ThreadID>), FSError> {
        let path_string = path.clone().as_string();
        let command_line = if args.is_empty() {
            vec![path_string.clone()]
        } else {
            args.clone()
        };
        let mut next_addrspace = AddrSpace::default();
        let mut next_fd_table = self.fd_table.clone();
        close_cloexec_fd_entries(&mut next_fd_table);
        let pid = self.pid.0;

        let next_snapshot = with_profiling(
            || {
                setup_process(
                    path.clone(),
                    args,
                    env,
                    &mut next_addrspace,
                    &mut next_fd_table,
                )
            },
            alloc::format!(
                "execve setup_process pid={} path={}",
                self.pid.0,
                path_string
            )
            .as_str(),
        )?;

        // TODO: kill all the other threads when execveing
        with_profiling(
            || {
                detach_all_process_mappings(self);
                let mut old_addrspace = mem::replace(&mut self.addrspace, next_addrspace);
                if self.borrowed_addrspace_from_parent {
                    self.borrowed_addrspace_from_parent = false;
                    if let Some(parent) = self.parent.clone() {
                        parent.lock().addrspace = old_addrspace;
                    } else {
                        old_addrspace.clean();
                    }
                } else {
                    old_addrspace.clean();
                }
            },
            alloc::format!(
                "execve clean addrspace pid={} path={}",
                pid,
                path_string
            )
            .as_str(),
        );

        let thread = current_thread();

        //thread_manager.kill_all_except(thread.clone());

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

        let mut thread_locked = thread.lock();

        self.fd_table = next_fd_table;
        thread_locked.snapshot = next_snapshot;
        thread_locked.kernel_stack_top = self.kernel_stack_top.as_u64();
        thread_locked.snapshot_state = SnapshotState::Normal;
        thread_locked.sig_handler_snapshot = ThreadSnapshot::default();
        thread_locked.saved_blocked_signals.clear();
        thread_locked.clear_child_tid = 0;
        thread_locked.robust_list_head = 0;
        thread_locked.robust_list_len = 0;
        thread_locked.rseq_area = 0;
        thread_locked.rseq_len = 0;
        thread_locked.rseq_flags = 0;
        thread_locked.rseq_sig = 0;
        thread_locked.last_user_snapshot = thread_locked.snapshot.inner;
        thread_locked.last_user_fs_base = thread_locked.snapshot.fs_base;
        self.pending_signals = Signals::default();
        self.pending_signal_info.fill(None);
        self.signal_actions = execve_signal_actions(&self.signal_actions);
        self.program_break = 0;
        self.command_line = command_line;
        self.sysv_shm_mappings.clear();

        with_profiling(
            || self.addrspace.load(),
            alloc::format!(
                "execve addrspace.load pid={} path={}",
                self.pid.0,
                path_string
            )
            .as_str(),
        );
        set_current_kernel_stack(thread_locked.kernel_stack_top);
        let vfork_blocker = self.vfork_blocker.take();
        Ok((
            &mut thread_locked.snapshot as *mut ThreadSnapshot,
            vfork_blocker,
        ))
    }
}

pub fn execve(path: Path, args: Vec<String>, env: Vec<String>) -> Result<(), FSError> {
    let (_, resolved_path) = VirtualFS.lock().resolve_with_path(path)?;
    let (snapshot, vfork_blocker) = {
        let _manager = MANAGER.lock();
        let current = current_process();
        with_profiling(
            || current.lock().execve(resolved_path, args, env),
            "process::execve total",
        )?
    };
    if let Some(thread_id) = vfork_blocker {
        wake_vfork_blocker(thread_id);
    }

    maybe_stop_current_after_exec();

    unsafe { (*snapshot).switch_from(None, None) };

    unreachable!();
}
