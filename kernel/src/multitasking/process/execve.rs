use alloc::{string::String, vec::Vec};

use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS, vfs_operations::read_all},
    multitasking::{
        MANAGER,
        process::{
            Process,
            misc::{init_objects, init_stack_layout},
        },
        scheduling::return_to_executor_no_save,
        thread::{
            THREAD_MANAGER,
            snapshot::{ThreadSnapshot, ThreadSnapshotType},
            thread::Thread,
        },
    },
    userspace::elf_loader::load_elf,
};

impl Process {
    fn execve(&mut self, path: Path, args: Vec<String>) -> Result<*mut ThreadSnapshot, FSError> {
        // TODO: kill all the other threads when execveing
        log::info!("execve: start");
        self.addrspace.clean();

        log::debug!("execve: locking thread manager");
        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();
        log::debug!("execve: thread manager locked");

        let thread = thread_manager.current.clone().unwrap();

        log::debug!("execve: kill all except current");
        //thread_manager.kill_all_except(thread.clone());
        log::debug!("execve: kill all done");

        let program = read_all(path.clone())?;

        let mut stack_builder = self.addrspace.allocate_user(16).1;
        let program = load_elf(&mut self.addrspace, &program);

        // Reallocates the kernel stack top (just in case)
        self.kernel_stack_top = self.addrspace.allocate_kernel(16).1.finish();

        assert!(!program.is_pie(), "Pie program is not supported for now");

        init_stack_layout(&mut stack_builder, &program);

        log::debug!("execve: locking current thread");
        let mut thread_locked = thread.lock();
        log::debug!("execve: current thread locked");

        thread_locked.snapshot = ThreadSnapshot::new(
            program.entry_point() as u64,
            &mut self.addrspace,
            stack_builder.finish().as_u64(),
            ThreadSnapshotType::Thread,
        );

        init_objects(&mut self.objects);
        self.addrspace.load();

        Ok(&mut thread_locked.snapshot as *mut ThreadSnapshot)
    }
}

pub fn execve(path: Path, args: Vec<String>) -> Result<(), FSError> {
    let snapshot = {
        log::debug!("execve: locking process manager");
        let manager = MANAGER.lock();
        log::debug!("execve: process manager locked");
        let current = manager.current.clone().unwrap();
        current.lock().execve(path, args)?
    };

    unsafe { (*snapshot).switch_from(None, None) };
    panic!("What the fuck")
}
