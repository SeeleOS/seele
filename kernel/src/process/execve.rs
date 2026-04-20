use alloc::{string::String, vec, vec::Vec};
use x86_64::VirtAddr;

use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS},
    misc::time::with_profiling,
    process::{Process, manager::MANAGER, new::setup_process},
    signal::{
        Signals,
        action::{SignalAction, SignalHandlingType},
        misc::default_signal_action_vec,
    },
    systemcall::handling::register_traced_process,
    thread::{
        THREAD_MANAGER, misc::SnapshotState, snapshot::ThreadSnapshot, stack::allocate_kernel_stack,
    },
    tss::TSS,
};

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

fn should_trace_exec_path(path: &str) -> bool {
    matches!(
        path,
        p if p.ends_with("/systemd-modules-load")
            || p.ends_with("/modprobe")
            || p.ends_with("/kmod")
            || p.ends_with("/systemd-executor")
            || p.ends_with("/systemd-tmpfiles")
            || p.ends_with("/systemd-sysusers")
            || p.ends_with("/systemd-journald")
            || p.ends_with("/systemd-userdbd")
            || p.ends_with("/udevadm")
            || p.ends_with("/systemd-udevd")
    )
}

impl Process {
    fn execve(
        &mut self,
        path: Path,
        args: Vec<String>,
        env: Vec<String>,
    ) -> Result<*mut ThreadSnapshot, FSError> {
        let path_string = path.clone().as_string();
        let command_line = if args.is_empty() {
            vec![path_string.clone()]
        } else {
            args.clone()
        };
        if should_trace_exec_path(&path_string) {
            register_traced_process(self.pid.0, path_string.clone());
        }
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

        self.close_cloexec_objects();
        thread_locked.snapshot = with_profiling(
            || {
                setup_process(
                    path,
                    args,
                    env,
                    &mut self.addrspace,
                    &mut self.objects,
                    &mut self.object_flags,
                )
            },
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
        unsafe {
            TSS.privilege_stack_table[0] = VirtAddr::new(thread_locked.kernel_stack_top);
        }

        Ok(&mut thread_locked.snapshot as *mut ThreadSnapshot)
    }
}

pub fn execve(path: Path, args: Vec<String>, env: Vec<String>) -> Result<(), FSError> {
    let (_, resolved_path) = VirtualFS.lock().resolve_with_path(path)?;
    let snapshot = {
        let manager = MANAGER.lock();
        let current = manager.current.clone().unwrap();
        with_profiling(
            || current.lock().execve(resolved_path, args, env),
            "process::execve total",
        )?
    };

    unsafe { (*snapshot).switch_from(None, None) };

    unreachable!();
}
