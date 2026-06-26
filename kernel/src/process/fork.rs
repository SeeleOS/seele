use crate::memory::utils::Mut;
use crate::{
    ipc::sysv_shm::inherit_forked_mappings,
    process::{Process, ProcessRef, clone_fd_table, clone_fs_context, misc::ProcessID},
    signal::Signal,
    thread::{ThreadRef, get_current_thread, misc::ThreadID, yielding::BlockType},
};
use alloc::sync::Arc;

impl Process {
    fn fork_process(
        parent: ProcessRef,
        share_fd_table: bool,
        share_fs_context: bool,
        share_addrspace: bool,
    ) -> (ProcessID, ProcessRef) {
        let (pid, new_process, inherited_shm_mappings) = {
            let current_thread = get_current_thread();
            let mut parent_locked = parent.lock();
            log::debug!(
                "Forking. Parent Current RSP: {:x}",
                current_thread.lock().snapshot.inner.rsp
            );
            let pid = ProcessID::new();

            log::debug!("fork: parent {} -> child {}", parent_locked.pid.0, pid.0);
            let inherited_shm_mappings = parent_locked.sysv_shm_mappings.clone();
            let child_fd_table = if share_fd_table {
                parent_locked.fd_table.clone()
            } else {
                clone_fd_table(&parent_locked.fd_table)
            };
            let child_fs_context = if share_fs_context {
                parent_locked.fs_context.clone()
            } else {
                clone_fs_context(&parent_locked.fs_context)
            };
            let child_addrspace = if share_addrspace {
                parent_locked.addrspace.clone_shared_vm()
            } else {
                parent_locked.addrspace.clone_all()
            };
            let child_pid_namespace = parent_locked
                .pending_child_pid_namespace
                .take()
                .unwrap_or_else(|| parent_locked.pid_namespace.clone());
            let child_pid_namespace_local_pid =
                if Arc::ptr_eq(&child_pid_namespace, &parent_locked.pid_namespace) {
                    parent_locked.pid_namespace_local_pid
                } else {
                    Some(1)
                };

            let new_process = Arc::new(Mut::new(Self {
                pid,
                pending_signals: parent_locked.pending_signals,
                pending_signal_info: parent_locked.pending_signal_info.clone(),
                addrspace: child_addrspace,
                kernel_stack_top: parent_locked.kernel_stack_top,
                fd_table: child_fd_table,
                fs_context: child_fs_context,
                command_line: parent_locked.command_line.clone(),
                parent: Some(parent.clone()),
                signal_actions: parent_locked.signal_actions.clone(),
                group_id: parent_locked.group_id,
                session_id: parent_locked.session_id,
                controlling_terminal: parent_locked.controlling_terminal,
                program_break: parent_locked.program_break,
                program_break_base: parent_locked.program_break_base,
                real_uid: parent_locked.real_uid,
                effective_uid: parent_locked.effective_uid,
                saved_uid: parent_locked.saved_uid,
                fs_uid: parent_locked.fs_uid,
                real_gid: parent_locked.real_gid,
                effective_gid: parent_locked.effective_gid,
                saved_gid: parent_locked.saved_gid,
                fs_gid: parent_locked.fs_gid,
                supplementary_groups: parent_locked.supplementary_groups.clone(),
                user_namespace_uid_map: parent_locked.user_namespace_uid_map.clone(),
                user_namespace_gid_map: parent_locked.user_namespace_gid_map.clone(),
                user_namespace_setgroups: parent_locked.user_namespace_setgroups.clone(),
                keep_capabilities: parent_locked.keep_capabilities,
                oom_score_adj: parent_locked.oom_score_adj,
                sched_policy: parent_locked.sched_policy,
                sched_priority: parent_locked.sched_priority,
                secure_bits: parent_locked.secure_bits,
                rlimit_nofile_cur: parent_locked.rlimit_nofile_cur,
                rlimit_nofile_max: parent_locked.rlimit_nofile_max,
                rlimit_memlock_cur: parent_locked.rlimit_memlock_cur,
                rlimit_memlock_max: parent_locked.rlimit_memlock_max,
                rlimit_rtprio_cur: parent_locked.rlimit_rtprio_cur,
                rlimit_rtprio_max: parent_locked.rlimit_rtprio_max,
                rlimit_core_cur: parent_locked.rlimit_core_cur,
                rlimit_core_max: parent_locked.rlimit_core_max,
                rlimit_fsize_cur: parent_locked.rlimit_fsize_cur,
                rlimit_fsize_max: parent_locked.rlimit_fsize_max,
                rlimit_nproc_cur: parent_locked.rlimit_nproc_cur,
                rlimit_nproc_max: parent_locked.rlimit_nproc_max,
                rlimit_data_cur: parent_locked.rlimit_data_cur,
                rlimit_data_max: parent_locked.rlimit_data_max,
                rlimit_stack_cur: parent_locked.rlimit_stack_cur,
                rlimit_stack_max: parent_locked.rlimit_stack_max,
                thread_keyring: 0,
                process_keyring: parent_locked.process_keyring,
                session_keyring: parent_locked.session_keyring,
                user_keyring: parent_locked.user_keyring,
                request_key_default_keyring: parent_locked.request_key_default_keyring,
                capability_effective: parent_locked.capability_effective,
                capability_permitted: parent_locked.capability_permitted,
                capability_inheritable: parent_locked.capability_inheritable,
                capability_bounding: parent_locked.capability_bounding,
                capability_ambient: parent_locked.capability_ambient,
                child_subreaper: false,
                child_exit_signal: Signal::SIGCHLD,
                dumpable: parent_locked.dumpable,
                no_new_privs: parent_locked.no_new_privs,
                net_namespace: parent_locked.net_namespace.clone(),
                ipc_namespace: parent_locked.ipc_namespace.clone(),
                mnt_namespace: parent_locked.mnt_namespace.clone(),
                pid_namespace: child_pid_namespace,
                pid_namespace_local_pid: child_pid_namespace_local_pid,
                pending_child_pid_namespace: None,
                user_namespace: parent_locked.user_namespace.clone(),
                uts_namespace: parent_locked.uts_namespace.clone(),
                sysv_shm_mappings: inherited_shm_mappings.clone(),
                ..Default::default()
            }));
            (pid, new_process, inherited_shm_mappings)
        };

        inherit_forked_mappings(&inherited_shm_mappings);
        (pid, new_process)
    }

    pub fn fork(parent: ProcessRef) -> (ProcessRef, ThreadRef) {
        Self::fork_with_sharing(parent, false, false, false)
    }

    pub fn fork_with_sharing(
        parent: ProcessRef,
        share_fd_table: bool,
        share_fs_context: bool,
        share_addrspace: bool,
    ) -> (ProcessRef, ThreadRef) {
        let (pid, new_process) =
            Self::fork_process(parent, share_fd_table, share_fs_context, share_addrspace);

        let current_thread = get_current_thread();
        let new_thread = current_thread
            .lock()
            .clone_and_spawn_with_id(new_process.clone(), ThreadID(pid.0));
        new_thread.lock().snapshot.inner.rax = 0;
        new_process.lock().threads.push(Arc::downgrade(&new_thread));

        let _ = pid;
        (new_process, new_thread)
    }

    pub fn vfork(parent: ProcessRef) -> (ProcessRef, ThreadRef) {
        Self::vfork_with_sharing(parent, false, false, true)
    }

    pub fn vfork_with_sharing(
        parent: ProcessRef,
        share_fd_table: bool,
        share_fs_context: bool,
        share_addrspace: bool,
    ) -> (ProcessRef, ThreadRef) {
        let (pid, new_process) = Self::fork_process(
            parent.clone(),
            share_fd_table,
            share_fs_context,
            share_addrspace,
        );

        let current_thread = get_current_thread();
        let new_thread = current_thread.lock().clone_and_spawn_blocked_with_id(
            new_process.clone(),
            ThreadID(pid.0),
            BlockType::Stopped,
        );
        new_thread.lock().snapshot.inner.rax = 0;
        new_process.lock().threads.push(Arc::downgrade(&new_thread));

        (new_process, new_thread)
    }

    pub fn restore_borrowed_addrspace_to_parent(&mut self) {
        if !self.borrowed_addrspace_from_parent {
            return;
        }

        let Some(parent) = self.parent.clone() else {
            self.borrowed_addrspace_from_parent = false;
            return;
        };

        let borrowed_addrspace = core::mem::take(&mut self.addrspace);
        self.borrowed_addrspace_from_parent = false;
        parent.lock().addrspace = borrowed_addrspace;
    }

    pub fn wake_vfork_child(thread: ThreadRef) {
        crate::thread::with_thread_manager(|manager| manager.wake(thread));
    }
}
