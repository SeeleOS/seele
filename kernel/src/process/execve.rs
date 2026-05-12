use core::mem;

use crate::{
    filesystem::{errors::FSError, path::Path, vfs_operations::resolve_path_with_final},
    ipc::sysv_shm::detach_all_process_mappings,
    memory::addrspace::AddrSpace,
    process::{
        Process, manager::wake_vfork_blocker, new::setup_process, new_fd_table,
        object::close_cloexec_fd_entries, ptrace::maybe_stop_current_after_exec,
    },
    signal::{
        Signals,
        action::{SignalAction, SignalHandlingType},
        misc::default_signal_action_vec,
    },
    smp::{current_process, current_thread, set_current_kernel_stack},
    thread::{
        extended_state::update_active_user_extended_state_ptr_for_thread,
        misc::{SnapshotState, ThreadID},
        snapshot::ThreadSnapshot,
        stack::allocate_kernel_stack,
        with_thread_manager,
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
    fn prepare_execve(
        &mut self,
        path: Path,
        args: Vec<String>,
        env: Vec<String>,
    ) -> Result<PreparedExecve, FSError> {
        let path_string = path.clone().as_string();
        let command_line = if args.is_empty() {
            vec![path_string.clone()]
        } else {
            args.clone()
        };
        let mut next_addrspace = AddrSpace::default();
        let mut next_fd_table = self.fd_table.lock().clone();
        close_cloexec_fd_entries(&mut next_fd_table);
        let next_snapshot = setup_process(
            path.clone(),
            args,
            env,
            &mut next_addrspace,
            &mut next_fd_table,
        )?;

        Ok(PreparedExecve {
            next_addrspace,
            next_fd_table,
            next_snapshot,
            command_line,
            threads: self.threads.clone(),
            borrowed_addrspace_from_parent: self.borrowed_addrspace_from_parent,
            parent: self.parent.clone(),
        })
    }

    fn commit_execve(
        &mut self,
        prepared: PreparedExecve,
        current_thread: crate::thread::ThreadRef,
    ) -> (*mut ThreadSnapshot, Option<ThreadID>) {
        {
            detach_all_process_mappings(self);
            let old_addrspace = mem::replace(&mut self.addrspace, prepared.next_addrspace);
            if prepared.borrowed_addrspace_from_parent {
                self.borrowed_addrspace_from_parent = false;
                if let Some(parent) = prepared.parent {
                    parent.lock().addrspace = old_addrspace;
                } else {
                    defer_exec_addrspace_cleanup(old_addrspace);
                }
            } else {
                defer_exec_addrspace_cleanup(old_addrspace);
            }
        }

        let thread = current_thread;

        //thread_manager.kill_all_except(thread.clone());

        // Reallocates the kernel stack top (just in case)
        self.kernel_stack_top = allocate_kernel_stack(16).finish();

        let mut thread_locked = thread.lock();

        let fd_table = new_fd_table();
        *fd_table.lock() = prepared.next_fd_table;
        self.fd_table = fd_table;
        thread_locked.snapshot = prepared.next_snapshot;
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
        update_active_user_extended_state_ptr_for_thread(&mut thread_locked);
        self.pending_signals = Signals::default();
        self.pending_signal_info.fill(None);
        self.signal_actions = execve_signal_actions(&self.signal_actions);
        self.program_break = 0;
        self.command_line = prepared.command_line;
        self.sysv_shm_mappings.clear();

        self.addrspace.load();
        set_current_kernel_stack(thread_locked.kernel_stack_top);
        let vfork_blocker = self.vfork_blocker.take();
        (
            &mut thread_locked.snapshot as *mut ThreadSnapshot,
            vfork_blocker,
        )
    }
}

struct PreparedExecve {
    next_addrspace: AddrSpace,
    next_fd_table: Vec<Option<crate::process::FdEntry>>,
    next_snapshot: ThreadSnapshot,
    command_line: Vec<String>,
    threads: Vec<alloc::sync::Weak<spin::Mutex<crate::thread::thread::Thread>>>,
    borrowed_addrspace_from_parent: bool,
    parent: Option<crate::process::ProcessRef>,
}

fn request_exec_siblings_to_exit(
    threads: Vec<alloc::sync::Weak<spin::Mutex<crate::thread::thread::Thread>>>,
    current_thread_id: ThreadID,
) {
    with_thread_manager(|manager| {
        manager.exit_thread_list_except(threads, current_thread_id);
    });
}

fn wait_for_exec_siblings_to_stop(
    threads: Vec<alloc::sync::Weak<spin::Mutex<crate::thread::thread::Thread>>>,
    current_thread_id: ThreadID,
) {
    while !with_thread_manager(|manager| {
        manager.exit_thread_list_except(threads.clone(), current_thread_id)
    }) {
        core::hint::spin_loop();
    }
}

fn defer_exec_addrspace_cleanup(_addrspace: AddrSpace) {
    // Other CPUs may still be returning from sibling threads while execve is
    // replacing the process address space. Dropping the old page table here can
    // leave those CPUs running with a reclaimed CR3; keep it alive until this
    // path grows a real cross-CPU quiescence/RCU mechanism.
}

pub fn execve(path: Path, args: Vec<String>, env: Vec<String>) -> Result<(), FSError> {
    let (_, resolved_path) = resolve_path_with_final(path, true)?;
    let current_thread = current_thread();
    let current_thread_id = current_thread.lock().id;
    let current = current_process();
    let prepared = current.lock().prepare_execve(resolved_path, args, env)?;
    request_exec_siblings_to_exit(prepared.threads.clone(), current_thread_id);
    wait_for_exec_siblings_to_stop(prepared.threads.clone(), current_thread_id);
    let (snapshot, vfork_blocker) = { current.lock().commit_execve(prepared, current_thread) };
    if let Some(thread_id) = vfork_blocker {
        wake_vfork_blocker(thread_id);
    }

    maybe_stop_current_after_exec();

    unsafe { (*snapshot).switch_from(None, None) };

    unreachable!();
}
