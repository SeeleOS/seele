use core::sync::atomic::AtomicU64;

use alloc::{string::String, vec::Vec};
use elfloader::LoadedElf;

use crate::{
    filesystem::{absolute_path::AbsolutePath, errors::FSError, vfs::VirtualFS},
    misc::stack_builder::StackBuilder,
    process::Process,
};

impl Process {
    pub fn change_directory(&mut self, directory: AbsolutePath) -> Result<(), FSError> {
        if directory.is_valid(VirtualFS.lock().root.clone().unwrap()) {
            self.current_directory = directory;
            Ok(())
        } else {
            Err(FSError::NotFound)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessID(pub u64);

impl Default for ProcessID {
    fn default() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        Self(NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

pub fn init_stack_layout(
    builder: &mut StackBuilder,
    file: &LoadedElf,
    interpreter_base: Option<u64>,
    args: Vec<String>,
    env_vars: Vec<String>,
) {
    let mut arg_ptrs = Vec::new();
    let mut env_ptrs = Vec::new();

    args.iter().for_each(|f| arg_ptrs.push(builder.push_str(f)));
    env_vars
        .iter()
        .for_each(|f| env_ptrs.push(builder.push_str(f)));

    let aux_entries = if interpreter_base.is_some() { 7 } else { 6 };
    let aux_bytes = aux_entries * 2 * 8;
    let argv_env_bytes = (arg_ptrs.len() + env_ptrs.len() + 3) as u64 * 8;
    builder.align_for_pushes(aux_bytes + argv_env_bytes, 16);

    builder.push_aux_entries(file, interpreter_base);

    builder.push(0); // envp terminator
    env_ptrs.iter().rev().for_each(|f| builder.push(*f));

    // argv
    builder.push(0); // argv terminator
    arg_ptrs.iter().rev().for_each(|f| builder.push(*f));

    // argc
    builder.push(args.len() as u64);
}
