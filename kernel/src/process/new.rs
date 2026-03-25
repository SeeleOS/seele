use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;

use crate::{
    filesystem::{
        absolute_path::AbsolutePath, errors::FSError, path::Path, vfs_operations::read_all,
    },
    memory::addrspace::AddrSpace,
    object::Object,
    process::{
        Process, ProcessRef,
        misc::{ProcessID, init_stack_layout},
        object::init_objects,
    },
    thread::{
        THREAD_MANAGER,
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        thread::Thread,
    },
    userspace::elf_loader::load_elf,
};

const DEFAULT_PATH: &str = "PATH=/programs";
const DEFAULT_TERM: &str = "TERM=xterm-256color";
const INIT_PATH: &str = "/programs/bash";

impl Process {
    pub fn init() -> ProcessRef {
        let pid = ProcessID::default();
        let mut addrspace = AddrSpace::default();
        let kernel_stack_top = addrspace.allocate_kernel(16).1.finish();

        let process_arc = Arc::new(Mutex::new(Process {
            pid,
            addrspace,
            kernel_stack_top,
            current_directory: AbsolutePath::default(),
            threads: Vec::new(),
            exit_code: None,
            objects: Vec::new(),
            parent: None,
        }));

        let process = &mut *process_arc.lock();

        log::debug!("process {}: setup start", pid.0);
        let context = setup_process(
            Path::new(INIT_PATH),
            Vec::new(),
            alloc::vec![DEFAULT_PATH.to_string(), DEFAULT_TERM.to_string()],
            &mut process.addrspace,
            &mut process.objects,
        )
        .unwrap();
        log::debug!("process {}: setup done", pid.0);

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
    args: Vec<String>,
    env: Vec<String>,
    addrspace: &mut AddrSpace,
    objects: &mut Vec<Option<Arc<dyn Object>>>,
) -> Result<ThreadSnapshot, FSError> {
    let program = read_all(path.clone())?;
    log::debug!("setup_process: loaded {} bytes", program.len());

    let mut stack_builder = addrspace.allocate_user(32).1;
    let program = load_elf(addrspace, &program);
    log::debug!(
        "setup_process: ELF entry_point = {:#x}",
        program.entry_point()
    );

    assert!(!program.is_pie(), "Pie program is not supported for now");

    init_stack_layout(&mut stack_builder, &program, args, env);

    init_objects(objects);

    Ok(ThreadSnapshot::new(
        program.entry_point() as u64,
        addrspace,
        stack_builder.finish().as_u64(),
        ThreadSnapshotType::Thread,
    ))
}
