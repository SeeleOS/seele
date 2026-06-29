use core::mem;

use crate::{
    filesystem::{
        errors::FSError,
        path::Path,
        vfs_operations::{open_path, resolve_path_with_final},
    },
    ipc::sysv_shm::detach_all_process_mappings,
    memory::addrspace::AddrSpace,
    process::{
        Process,
        manager::wake_vfork_blocker,
        new::{SetupProcessRequest, setup_process},
        new_fd_table,
        object::close_cloexec_fd_entries,
        ptrace::maybe_stop_current_after_exec,
    },
    signal::{
        SIGNAL_AMOUNT, Signals,
        action::{SignalAction, SignalHandlingType},
        misc::default_signal_action_vec,
    },
    smp::{current_process, current_thread, set_current_kernel_stack},
    thread::{
        extended_state::update_active_user_extended_state_ptr_for_thread,
        misc::{SnapshotState, ThreadID},
        scheduling::return_to_scheduler_from_current,
        snapshot::ThreadSnapshot,
        thread::LinuxStack,
        with_thread_manager,
    },
};
use alloc::{string::String, vec, vec::Vec};

fn execve_signal_actions(old_actions: &[SignalAction]) -> [SignalAction; SIGNAL_AMOUNT] {
    let mut actions = default_signal_action_vec();
    for (index, old) in old_actions.iter().enumerate() {
        if matches!(old.handling_type, SignalHandlingType::Ignore) {
            actions[index] = *old;
        }
    }
    actions
}

impl Process {
    fn prepare_execve(
        &mut self,
        path: Path,
        exec_path: Path,
        args: Vec<String>,
        env: Vec<String>,
    ) -> Result<PreparedExecve, FSError> {
        const S_ISUID: u32 = 0o4000;
        const S_ISGID: u32 = 0o2000;

        let path_string = exec_path.clone().as_string();
        let command_line = if args.is_empty() {
            vec![path_string.clone()]
        } else {
            args.clone()
        };
        let exec_info = open_path(path.clone())?.info()?;
        let setuid = (exec_info.permission.0 & S_ISUID != 0).then_some(exec_info.uid);
        let setgid = (exec_info.permission.0 & S_ISGID != 0).then_some(exec_info.gid);
        let mut next_addrspace = AddrSpace::default();
        let mut next_fd_table = self.fd_table.lock().clone();
        let fs_context = self.fs_context.lock().clone();
        let closed_fd_entries = close_cloexec_fd_entries(&mut next_fd_table);
        let next_snapshot = setup_process(
            SetupProcessRequest {
                path: path.clone(),
                exec_path,
                args,
                env,
                kernel_stack_top: crate::thread::get_current_thread().lock().kernel_stack_top,
            },
            &mut next_addrspace,
            &mut next_fd_table,
            &fs_context,
        )?;

        Ok(PreparedExecve {
            next_addrspace,
            next_fd_table,
            next_snapshot,
            exec_path: path,
            command_line,
            setuid,
            setgid,
            closed_fd_entries,
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
                    cleanup_exec_addrspace(old_addrspace);
                }
            } else {
                cleanup_exec_addrspace(old_addrspace);
            }
        }

        let thread = current_thread;

        //thread_manager.kill_all_except(thread.clone());

        let mut thread_locked = thread.lock();
        self.kernel_stack_top = x86_64::VirtAddr::new(thread_locked.kernel_stack_top);

        let fd_table = new_fd_table();
        *fd_table.lock() = prepared.next_fd_table;
        self.fd_table = fd_table;
        for entry in prepared.closed_fd_entries {
            crate::process::object::release_fd_entry_resources(self.pid, &entry);
        }
        thread_locked.snapshot = prepared.next_snapshot;
        thread_locked.snapshot_state = SnapshotState::Normal;
        thread_locked.sig_handler_snapshot = ThreadSnapshot::default();
        thread_locked.sigaltstack = LinuxStack::default();
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
        self.program_break_base = 0;
        self.command_line = prepared.command_line;
        self.exec_path = prepared.exec_path;
        self.sysv_shm_mappings.clear();
        let old_effective_uid = self.effective_uid;
        let gained_root = prepared.setuid == Some(0) && old_effective_uid != 0;
        if let Some(uid) = prepared.setuid {
            self.effective_uid = uid;
            self.saved_uid = uid;
            self.fs_uid = uid;
        }
        if let Some(gid) = prepared.setgid {
            self.effective_gid = gid;
            self.saved_gid = gid;
            self.fs_gid = gid;
        }
        self.update_exec_uid_capabilities(old_effective_uid, gained_root);

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
    exec_path: Path,
    command_line: Vec<String>,
    setuid: Option<u32>,
    setgid: Option<u32>,
    closed_fd_entries: Vec<crate::process::FdEntry>,
    threads: Vec<alloc::sync::Weak<crate::memory::utils::Mut<crate::thread::thread::Thread>>>,
    borrowed_addrspace_from_parent: bool,
    parent: Option<crate::process::ProcessRef>,
}

fn request_exec_siblings_to_exit(
    threads: Vec<alloc::sync::Weak<crate::memory::utils::Mut<crate::thread::thread::Thread>>>,
    current_thread_id: ThreadID,
) {
    with_thread_manager(|manager| {
        manager.exit_thread_list_except(threads, current_thread_id);
    });
}

fn wait_for_exec_siblings_to_stop(
    threads: Vec<alloc::sync::Weak<crate::memory::utils::Mut<crate::thread::thread::Thread>>>,
    current_thread_id: ThreadID,
) {
    while !with_thread_manager(|manager| {
        manager.exit_thread_list_except(threads.clone(), current_thread_id)
    }) {
        return_to_scheduler_from_current();
    }
}

fn cleanup_exec_addrspace(mut addrspace: AddrSpace) {
    addrspace.clean();
}

pub fn execve(path: Path, args: Vec<String>, env: Vec<String>) -> Result<(), FSError> {
    let exec_path = path.clone();
    let (_, resolved_path) = resolve_path_with_final(path, true)?;
    let current_thread = current_thread();
    let current_thread_id = current_thread.lock().id;
    let current = current_process();
    let prepared = current
        .lock()
        .prepare_execve(resolved_path, exec_path, args, env)?;
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
