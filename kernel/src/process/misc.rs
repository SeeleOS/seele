use core::sync::atomic::{AtomicU64, Ordering};

use alloc::{string::String, vec::Vec};

use crate::{
    define_with_accessor,
    elfloader::ElfInfo,
    filesystem::{absolute_path::AbsolutePath, errors::FSError, vfs::VirtualFS},
    misc::{stack_builder::StackBuilder, time::Time},
    process::{
        Process, ProcessRef,
        manager::{MANAGER, get_current_process},
    },
    systemcall::utils::{SyscallError, SyscallResult},
    thread::{THREAD_MANAGER, misc::State, yielding::BlockType},
};

impl Process {
    pub fn have_exited(&self) -> bool {
        self.exit_code.is_some()
    }

    pub fn change_directory(&mut self, directory: AbsolutePath) -> Result<(), FSError> {
        if VirtualFS.lock().resolve_dir(directory.as_normal()).is_ok() {
            self.current_directory = directory;
            Ok(())
        } else {
            Err(FSError::NotADirectory)
        }
    }

    pub fn wake_blocked_threads(&self) {
        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
        for weak in &self.threads {
            let Some(thread) = weak.upgrade() else {
                continue;
            };

            if matches!(thread.lock().state, State::Blocked(_))
                && !matches!(thread.lock().state, State::Blocked(BlockType::Stopped))
            {
                thread_manager.wake(thread.clone());
            }
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessID(pub u64);

pub(crate) fn next_linux_task_id() -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

impl ProcessID {
    pub fn new() -> Self {
        Self(next_linux_task_id())
    }
}

const PLATFORM: &str = "x86_64";
const USER_STACK_HEADROOM_BYTES: u64 = 4096;
const DEFAULT_USER_STACK_PAGES: u64 = 256;

pub fn user_stack_pages_for_exec(
    exec_path: &str,
    args: &[String],
    env_vars: &[String],
    has_interpreter: bool,
) -> u64 {
    let required_bytes = required_initial_stack_bytes(exec_path, args, env_vars, has_interpreter);
    let required_pages = (required_bytes + USER_STACK_HEADROOM_BYTES).div_ceil(4096);

    DEFAULT_USER_STACK_PAGES.max(required_pages)
}

fn required_initial_stack_bytes(
    exec_path: &str,
    args: &[String],
    env_vars: &[String],
    has_interpreter: bool,
) -> u64 {
    let strings_bytes = args.iter().map(|arg| (arg.len() + 1) as u64).sum::<u64>()
        + env_vars
            .iter()
            .map(|env| (env.len() + 1) as u64)
            .sum::<u64>()
        + (exec_path.len() + 1) as u64
        + (PLATFORM.len() + 1) as u64;
    let random_bytes = core::mem::size_of::<[u64; 2]>() as u64;
    let aux_entries = if has_interpreter { 20 } else { 19 };
    let aux_bytes = aux_entries * 2 * 8;
    let argv_env_bytes = (args.len() + env_vars.len() + 3) as u64 * 8;
    let alignment_slack = 15;

    strings_bytes + random_bytes + aux_bytes + argv_env_bytes + alignment_slack
}

pub fn init_stack_layout(
    builder: &mut StackBuilder,
    file: &ElfInfo,
    interpreter_base: Option<u64>,
    exec_path: &str,
    args: Vec<String>,
    env_vars: Vec<String>,
) {
    let mut arg_ptrs = Vec::new();
    let mut env_ptrs = Vec::new();

    args.iter().for_each(|f| arg_ptrs.push(builder.push_str(f)));
    env_vars
        .iter()
        .for_each(|f| env_ptrs.push(builder.push_str(f)));

    let execfn_ptr = builder.push_str(exec_path);
    let platform_ptr = builder.push_str("x86_64");
    let random_bytes = [
        Time::current().as_nanoseconds(),
        Time::since_boot().as_nanoseconds(),
    ];
    let random_ptr = builder.push_struct(&random_bytes);

    let aux_entries = if interpreter_base.is_some() { 20 } else { 19 };
    let aux_bytes = aux_entries * 2 * 8;
    let argv_env_bytes = (arg_ptrs.len() + env_ptrs.len() + 3) as u64 * 8;
    builder.align_for_pushes(aux_bytes + argv_env_bytes, 16);

    builder.push_aux_entries(file, interpreter_base, execfn_ptr, platform_ptr, random_ptr);

    builder.push(0); // envp terminator
    env_ptrs.iter().rev().for_each(|f| builder.push(*f));

    // argv
    builder.push(0); // argv terminator
    arg_ptrs.iter().rev().for_each(|f| builder.push(*f));

    // argc
    builder.push(args.len() as u64);
}

define_with_accessor!("current_process", Process, get_current_process);

pub fn get_process_with_pid(pid: ProcessID) -> SyscallResult<ProcessRef> {
    MANAGER
        .lock()
        .processes
        .get(&pid)
        .ok_or(SyscallError::NoProcess)
        .cloned()
}
