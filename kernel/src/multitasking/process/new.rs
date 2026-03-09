use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    filesystem::{errors::FSError, path::Path, vfs::VirtualFS, vfs_operations::read_all},
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
    object::Object,
    userspace::elf_loader::load_elf,
};

impl Process {
    pub fn new(path: Path) -> ProcessRef {
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

        let process = &mut *process_arc.lock();

        let context = setup_process(path, &mut process.addrspace, &mut process.objects).unwrap();

        // Initilizes the main thread
        process
            .threads
            .push(Arc::downgrade(&THREAD_MANAGER.get().unwrap().lock().spawn(
                Thread::from_snapshot(context, process_arc.clone(), kernel_stack_top.as_u64()),
            )));

        process_arc.clone()
    }
}

pub fn setup_process(
    path: Path,
    addrspace: &mut AddrSpace,
    objects: &mut Vec<Option<Arc<dyn Object>>>,
) -> Result<ThreadSnapshot, FSError> {
    let program = read_all(path.clone())?;

    let mut stack_builder = addrspace.allocate_user(16).1;
    let program = load_elf(addrspace, &program);

    assert!(!program.is_pie(), "Pie program is not supported for now");

    init_stack_layout(&mut stack_builder, &program);

    init_objects(objects);

    Ok(ThreadSnapshot::new(
        program.entry_point() as u64,
        addrspace,
        stack_builder.finish().as_u64(),
        ThreadSnapshotType::Thread,
    ))
}
