use alloc::{string::String, vec::Vec};

use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS},
    multitasking::{
        MANAGER,
        process::{Process, new::setup_process},
        thread::{
            THREAD_MANAGER,
            snapshot::ThreadSnapshot,
        },
    },
};

impl Process {
    fn execve(
        &mut self,
        path: Path,
        args: Vec<String>,
        env: Vec<String>,
    ) -> Result<*mut ThreadSnapshot, FSError> {
        if !path.is_valid(VirtualFS.lock().root.clone().unwrap()) {
            return Err(FSError::NotFound);
        }
        // TODO: kill all the other threads when execveing
        log::trace!("execve: start {}", path.clone().as_string());
        self.addrspace.clean();

        log::trace!("execve: locking thread manager");
        let thread_manager = THREAD_MANAGER.get().unwrap().lock();
        log::trace!("execve: thread manager locked");

        let thread = thread_manager.current.clone().unwrap();

        log::trace!("execve: kill all except current");
        //thread_manager.kill_all_except(thread.clone());
        log::trace!("execve: kill all done");

        // Reallocates the kernel stack top (just in case)
        self.kernel_stack_top = self.addrspace.allocate_kernel(16).1.finish();

        log::trace!("execve: locking current thread");
        let mut thread_locked = thread.lock();
        log::trace!("execve: current thread locked");

        thread_locked.snapshot =
            setup_process(path, args, env, &mut self.addrspace, &mut self.objects).unwrap();

        self.addrspace.load();

        Ok(&mut thread_locked.snapshot as *mut ThreadSnapshot)
    }
}

pub fn execve(path: Path, args: Vec<String>, env: Vec<String>) -> Result<(), FSError> {
    let snapshot = {
        log::debug!("execve: locking process manager");
        let manager = MANAGER.lock();
        log::debug!("execve: process manager locked");
        let current = manager.current.clone().unwrap();
        current.lock().execve(path, args, env)?
    };

    unsafe { (*snapshot).switch_from(None, None) };
    panic!("What the fuck")
}
