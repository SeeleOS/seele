use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    filesystem::path::Path,
    memory::{addrspace::AddrSpace, page_table_wrapper::PageTableWrapped},
    multitasking::{
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
        let mut addrspace = AddrSpace::default();
        let kernel_stack_top = addrspace.allocate_kernel(16).1.finish();

        let process_arc = Arc::new(Mutex::new(Process {
            pid,
            addrspace,
            kernel_stack_top,
            current_directory: Path::default(),
            threads: Vec::new(),
            objects: Vec::new(),
        }));

        let mut process = process_arc.lock();

        let mut stack_builder = process.addrspace.allocate_user(16).1;
        let program = load_elf(&mut process.addrspace, program);

        assert!(!program.is_pie(), "Pie program is not supported for now");

        init_stack_layout(&mut stack_builder, &program);

        let context = ThreadSnapshot::new(
            program.entry_point() as u64,
            &mut process.addrspace,
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
