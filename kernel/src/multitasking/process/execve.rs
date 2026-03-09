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
    s_println,
    userspace::elf_loader::load_elf,
};

impl Process {
    fn execve(&mut self, path: Path, args: Vec<String>) -> Result<*mut ThreadSnapshot, FSError> {
        // TODO: kill all the other threads when execveing
        s_println!("in execve");
        self.addrspace.clean();

        let mut thread_manager = THREAD_MANAGER.get().unwrap().lock();

        let thread = thread_manager.current.clone().unwrap();

        thread_manager.kill_all_except(thread.clone());

        let mut program = alloc::vec![0u8; VirtualFS.lock().file_info(path.clone())?.size];
        read_all(path.clone(), &mut program)?;

        let mut stack_builder = self.addrspace.allocate_user(16).1;
        let program = load_elf(&mut self.addrspace, &program);

        // Reallocates the kernel stack top (just in case)
        self.kernel_stack_top = self.addrspace.allocate_kernel(16).1.finish();

        assert!(!program.is_pie(), "Pie program is not supported for now");

        init_stack_layout(&mut stack_builder, &program);

        let mut thread_locked = thread.lock();

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
        let manager = MANAGER.lock();
        let current = manager.current.clone().unwrap();
        current.lock().execve(path, args)?
    };

    unsafe { (*snapshot).switch_from(None, None) };
    panic!("What the fuck")
}
