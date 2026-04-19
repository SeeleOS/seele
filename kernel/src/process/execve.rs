use alloc::{string::String, vec::Vec};
use x86_64::VirtAddr;

use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS},
    misc::time::with_profiling,
    process::{Process, manager::MANAGER, new::setup_process},
    s_println,
    signal::{
        Signals,
        action::{SignalAction, SignalHandlingType},
        misc::default_signal_action_vec,
    },
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

impl Process {
    fn execve(
        &mut self,
        path: Path,
        args: Vec<String>,
        env: Vec<String>,
    ) -> Result<*mut ThreadSnapshot, FSError> {
        let trace_exec = self.pid.0 <= 32;
        let path_string = path.clone().as_string();
        if trace_exec {
            s_println!("execve stage pid={} start path={}", self.pid.0, path_string);
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
        if trace_exec {
            s_println!("execve stage pid={} clean-done", self.pid.0);
        }

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
        if trace_exec {
            s_println!("execve stage pid={} kernel-stack-done", self.pid.0);
        }

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
        if trace_exec {
            s_println!(
                "execve stage pid={} setup-done rip={:#x} rsp={:#x} cs={:#x} ss={:#x}",
                self.pid.0,
                thread_locked.snapshot.inner.rip,
                thread_locked.snapshot.inner.rsp,
                thread_locked.snapshot.inner.cs,
                thread_locked.snapshot.inner.ss,
            );
        }
        thread_locked.kernel_stack_top = self.kernel_stack_top.as_u64();
        thread_locked.snapshot_state = SnapshotState::Normal;
        thread_locked.sig_handler_snapshot = ThreadSnapshot::default();
        thread_locked.saved_blocked_signals.clear();
        self.pending_signals = Signals::default();
        self.signal_actions = execve_signal_actions(&self.signal_actions);
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
        if trace_exec {
            s_println!("execve stage pid={} load-done", self.pid.0);
        }
        unsafe {
            TSS.privilege_stack_table[0] = VirtAddr::new(thread_locked.kernel_stack_top);
        }
        if trace_exec {
            s_println!(
                "execve stage pid={} ready-switch kernel_rsp={:#x}",
                self.pid.0,
                thread_locked.kernel_stack_top
            );
        }

        Ok(&mut thread_locked.snapshot as *mut ThreadSnapshot)
    }
}

pub fn execve(path: Path, args: Vec<String>, env: Vec<String>) -> Result<(), FSError> {
    crate::s_println!("execve fn enter");
    let current = crate::process::manager::get_current_process();
    crate::s_println!("execve fn got-current-ref");
    let trace_exec = current.lock().pid.0 <= 32;
    if trace_exec {
        crate::s_println!("execve fn got-current-lock");
    }
    let (_, resolved_path) = VirtualFS.lock().resolve_with_path(path)?;
    if trace_exec {
        crate::s_println!(
            "execve fn resolved-path {}",
            resolved_path.clone().as_string()
        );
    }
    let snapshot = {
        log::debug!("execve: locking process manager");
        let manager = MANAGER.lock();
        if trace_exec {
            crate::s_println!("execve fn manager-locked");
        }
        log::debug!("execve: process manager locked");
        let current = manager.current.clone().unwrap();
        if trace_exec {
            crate::s_println!("execve fn got-manager-current");
        }
        with_profiling(
            || current.lock().execve(resolved_path, args, env),
            "process::execve total",
        )?
    };
    if trace_exec {
        unsafe {
            s_println!(
                "execve stage switch-call rip={:#x} rsp={:#x} cs={:#x} ss={:#x}",
                (*snapshot).inner.rip,
                (*snapshot).inner.rsp,
                (*snapshot).inner.cs,
                (*snapshot).inner.ss,
            );
        }
    }

    unsafe { (*snapshot).switch_from(None, None) };

    unreachable!();
}
