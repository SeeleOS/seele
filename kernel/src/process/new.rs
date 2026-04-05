use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::{mem::size_of, slice};
use elfloader::{ElfBinary, LoadedElf};
use seele_sys::signal::Signals;
use spin::Mutex;

use crate::{
    filesystem::{
        absolute_path::AbsolutePath, errors::FSError, path::Path, vfs_operations::read_all,
    },
    memory::addrspace::AddrSpace,
    object::{Object, tty_device::get_default_tty},
    process::{
        Process, ProcessRef,
        misc::{ProcessID, init_stack_layout},
        object::init_objects,
    },
    signal::{SIGNAL_AMOUNT, action::SignalAction, misc::default_signal_action_vec},
    thread::{
        THREAD_MANAGER,
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        thread::Thread,
    },
    userspace::elf_loader::load_elf,
};

const DEFAULT_PATH: &str = "PATH=/programs";
const DEFAULT_TERM: &str = "TERM=xterm-256color";
const DEFAULT_HOME: &str = "HOME=/home";
const INIT_PATH: &str = "/programs/bash";
const MAX_SHEBANG_DEPTH: usize = 4;

struct AlignedElfBuffer {
    storage: Vec<u64>,
    len: usize,
}

impl AlignedElfBuffer {
    fn new(bytes: &[u8]) -> Self {
        let words = bytes.len().div_ceil(size_of::<u64>());
        let mut storage = vec![0u64; words];

        unsafe {
            let dst = slice::from_raw_parts_mut(storage.as_mut_ptr() as *mut u8, bytes.len());
            dst.copy_from_slice(bytes);
        }

        Self {
            storage,
            len: bytes.len(),
        }
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.storage.as_ptr() as *const u8, self.len) }
    }
}

fn interp_load_base(image: &LoadedElf, binary: &ElfBinary) -> u64 {
    image.program_header_table() - binary.program_header_table()
}

fn parse_shebang(program_bytes: &[u8]) -> Result<Option<(Path, Option<String>)>, FSError> {
    if !program_bytes.starts_with(b"#!") {
        return Ok(None);
    }

    let line_end = program_bytes
        .iter()
        .position(|&byte| byte == b'\n')
        .unwrap_or(program_bytes.len());
    let line = core::str::from_utf8(&program_bytes[2..line_end]).map_err(|_| FSError::Other)?;
    let line = line.trim().trim_end_matches('\r');

    if line.is_empty() {
        return Err(FSError::Other);
    }

    let mut parts = line.split_whitespace();
    let interpreter = parts.next().ok_or(FSError::Other)?;
    let optional_arg = parts.next().map(str::to_string);

    if parts.next().is_some() {
        return Err(FSError::Other);
    }

    Ok(Some((Path::new(interpreter), optional_arg)))
}

impl Process {
    pub fn init() -> ProcessRef {
        let pid = ProcessID::default();
        let mut addrspace = AddrSpace::default();
        let kernel_stack_top = addrspace.allocate_kernel(16).1.finish();

        let process_arc = Arc::new(Mutex::new(Process {
            pid,
            addrspace,
            kernel_stack_top,
            ..Default::default()
        }));

        let process = &mut *process_arc.lock();

        log::debug!("process {}: setup start", pid.0);
        let context = setup_process(
            Path::new(INIT_PATH),
            Vec::new(),
            alloc::vec![
                DEFAULT_PATH.into(),
                DEFAULT_TERM.into(),
                DEFAULT_HOME.into(),
            ],
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

        *get_default_tty().active_group.lock() = Some(process.group_id);

        process_arc.clone()
    }
}

fn setup_process_inner(
    path: Path,
    args: Vec<String>,
    env: Vec<String>,
    addrspace: &mut AddrSpace,
    objects: &mut Vec<Option<Arc<dyn Object>>>,
    shebang_depth: usize,
) -> Result<ThreadSnapshot, FSError> {
    if shebang_depth > MAX_SHEBANG_DEPTH {
        return Err(FSError::Other);
    }

    let path_string = path.clone().as_string();
    let program_bytes = read_all(path.clone())?;
    log::debug!("setup_process: loaded {} bytes", program_bytes.len());

    if let Some((interpreter, optional_arg)) = parse_shebang(&program_bytes)? {
        log::debug!(
            "setup_process: shebang {} -> {}",
            path_string,
            interpreter.clone().as_string()
        );

        let mut interpreter_args = Vec::with_capacity(args.len() + 2);
        interpreter_args.push(interpreter.clone().as_string());
        if let Some(optional_arg) = optional_arg {
            interpreter_args.push(optional_arg);
        }
        interpreter_args.push(path_string);
        interpreter_args.extend(args.into_iter().skip(1));

        return setup_process_inner(
            interpreter,
            interpreter_args,
            env,
            addrspace,
            objects,
            shebang_depth + 1,
        );
    }

    let program_bytes = AlignedElfBuffer::new(&program_bytes);

    let mut stack_builder = addrspace.allocate_user(32).1;
    let program_binary =
        ElfBinary::new(program_bytes.as_bytes()).expect("Failed to parse elf binary");
    let program = load_elf(addrspace, &program_binary);

    let (entry_point, interpreter_base) = match &program {
        LoadedElf::Basic(info) => (info.entry_point, None),
        LoadedElf::Dynamic(info) => {
            let interp_bytes = read_all(Path::new(info.interpreter))?;
            log::debug!(
                "setup_process: loaded interpreter {} ({} bytes)",
                info.interpreter,
                interp_bytes.len()
            );
            let interp_bytes = AlignedElfBuffer::new(&interp_bytes);
            let interp_binary =
                ElfBinary::new(interp_bytes.as_bytes()).expect("Failed to parse interpreter ELF");
            let interp = load_elf(addrspace, &interp_binary);
            let interp_base = interp_load_base(&interp, &interp_binary);
            (interp.entry_point(), Some(interp_base))
        }
    };

    init_stack_layout(&mut stack_builder, &program, interpreter_base, args, env);

    init_objects(objects);

    Ok(ThreadSnapshot::new(
        entry_point,
        addrspace,
        stack_builder.finish().as_u64(),
        ThreadSnapshotType::Thread,
    ))
}

pub fn setup_process(
    path: Path,
    mut args: Vec<String>,
    env: Vec<String>,
    addrspace: &mut AddrSpace,
    objects: &mut Vec<Option<Arc<dyn Object>>>,
) -> Result<ThreadSnapshot, FSError> {
    if args.first().is_none() {
        args.insert(0, path.clone().as_string());
    }

    setup_process_inner(path, args, env, addrspace, objects, 0)
}
