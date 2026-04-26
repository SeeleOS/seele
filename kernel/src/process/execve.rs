use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS},
    misc::time::with_profiling,
    process::{
        Process,
        manager::{MANAGER, wake_vfork_blocker},
        new::setup_process,
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
        // TODO: kill all the other threads when execveing
        with_profiling(
            || self.addrspace.clean(),
            alloc::format!(
                "execve clean addrspace pid={} path={}",
                self.pid.0,
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

        self.close_cloexec_objects();
        thread_locked.snapshot = with_profiling(
            || setup_process(path, args, env, &mut self.addrspace, &mut self.fd_table),
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
        self.pending_signals = Signals::default();
        self.signal_actions = execve_signal_actions(&self.signal_actions);
        self.program_break = 0;
        self.command_line = command_line;

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
        Ok((&mut thread_locked.snapshot as *mut ThreadSnapshot, vfork_blocker))
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

    unsafe { (*snapshot).switch_from(None, None) };

    unreachable!();
}
