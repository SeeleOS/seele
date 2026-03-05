use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    filesystem::path::Path,
    memory::{addrspace::AddrSpace, page_table_wrapper::PageTableWrapped},
    multitasking::{
        memory::{allocate_kernel_stack, allocate_stack},
        process::{
            Process, ProcessRef,
            misc::{ProcessID, init_objects, init_stack_layout},
        },
        thread::{
            THREAD_MANAGER,
            snapshot::{ThreadSnapshot, ThreadSnapshotType},
            thread::Thread,
        },
    },
    userspace::elf_loader::load_elf,
};

impl Process {
    pub fn new(program: &[u8]) -> ProcessRef {
        let pid = ProcessID::default();
        let mut page_table = PageTableWrapped::default();
        let kernel_stack_top = allocate_kernel_stack(160, &mut page_table.inner).finish();

        let process_arc = Arc::new(Mutex::new(Process {
            pid,
            addrspace: AddrSpace::default(),
            kernel_stack_top,
            used_memories: Vec::new(),
            current_directory: Path::default(),
            threads: Vec::new(),
            objects: Vec::new(),
        }));

        let mut process = process_arc.lock();

        let mut stack_builder = allocate_stack(160, &mut process.addrspace.page_table.inner);
        let program = load_elf(&mut process.addrspace.page_table, program);

        assert!(!program.is_pie(), "Pie program is not supported for now");

        init_stack_layout(&mut stack_builder, &program);

        let context = ThreadSnapshot::new(
            program.entry_point() as u64,
            &mut process.addrspace.page_table,
            stack_builder.finish().as_u64(),
            ThreadSnapshotType::Thread,
        );

        // Initilizes the main thread
        process
            .threads
            .push(Arc::downgrade(&THREAD_MANAGER.get().unwrap().lock().spawn(
                Thread::from_snapshot(context, process_arc.clone(), kernel_stack_top.as_u64()),
            )));

        init_objects(&mut process.objects);

        process_arc.clone()
    }
}
