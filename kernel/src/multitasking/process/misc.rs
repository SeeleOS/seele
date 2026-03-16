use core::sync::atomic::AtomicU64;

use alloc::{string::String, sync::Arc, vec::Vec};
use elfloader::ElfBinary;

use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS},
    misc::stack_builder::StackBuilder,
    multitasking::process::Process,
    object::{
        Object,
        error::ObjectError,
        misc::{ObjectRef, ObjectResult},
        tty_device::{TtyDevice, get_default_tty},
    },
};

impl Process {
    pub fn change_directory(&mut self, directory: Path) -> Result<(), FSError> {
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

pub fn init_objects(objects: &mut Vec<Option<Arc<dyn Object>>>) {
    objects.push(Some(get_default_tty())); // stdin (unimpllemented)
    objects.push(Some(get_default_tty())); // stdout
    objects.push(Some(get_default_tty())); // stderr
}

pub fn init_stack_layout(
    builder: &mut StackBuilder,
    file: &ElfBinary,
    args: Vec<String>,
    env_vars: Vec<String>,
) {
    let mut arg_ptrs = Vec::new();
    let mut env_ptrs = Vec::new();

    args.iter().for_each(|f| arg_ptrs.push(builder.push_str(f)));
    env_vars
        .iter()
        .for_each(|f| env_ptrs.push(builder.push_str(f)));

    // B. 使用你的 write_and_sub 按照 ABI 逆序压栈
    builder.push_aux_entries(file);

    builder.push(0); // envp terminator
    env_ptrs.iter().rev().for_each(|f| builder.push(*f));

    // argv
    builder.push(0); // argv terminator
    arg_ptrs.iter().rev().for_each(|f| builder.push(*f));

    // argc
    builder.push(args.len() as u64);
}

impl Process {
    pub fn get_object(&mut self, index: u64) -> ObjectResult<ObjectRef> {
        self.objects
            .get(index as usize)
            .ok_or(ObjectError::DoesNotExist)?
            .clone()
            .ok_or(ObjectError::DoesNotExist)
    }
}
