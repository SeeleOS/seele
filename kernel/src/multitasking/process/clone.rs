use crate::{
    memory::page_table_wrapper::PageTableWrapped,
    multitasking::process::{Process, misc::ProcessID},
};

impl Process {
    /// Clones a process, with all its memory (aka fork)
    pub fn process_clone(&mut self) -> Self {
        let page_table = PageTableWrapped::default();
        let new_pcd = Self {
            pid: ProcessID::default(),
            page_table,
            kernel_stack_top: self.kernel_stack_top,
            threads: self.threads.clone(),
            objects: self.objects.clone(),
            current_directory: self.current_directory.clone(),
        };

        unimplemented!();

        new_pcd
    }
}
