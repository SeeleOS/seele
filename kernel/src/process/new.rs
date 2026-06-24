use crate::memory::utils::Mut;
use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};

use crate::{
    elfloader::{load_elf_lazy, read_elf_header},
    filesystem::{
        absolute_path::AbsolutePath, errors::FSError, object::FileLikeObject, path::Path,
        vfs_operations::open_path,
    },
    memory::addrspace::AddrSpace,
    object::tty_device::get_default_tty,
    process::{
        FdEntry, FsContext, Process, ProcessRef,
        group::{ProcessGroupID, SessionID},
        misc::{ProcessID, init_stack_layout, user_stack_pages_for_exec},
        new_fd_table,
        object::init_objects,
    },
    thread::{
        misc::ThreadID,
        snapshot::{ThreadSnapshot, ThreadSnapshotType},
        stack::allocate_owned_kernel_stack,
        thread::Thread,
    },
};
use x86_64::{VirtAddr, structures::paging::Page};

const DEFAULT_PATH: &str = "PATH=/bin:/usr/bin";
const DEFAULT_TERM: &str = "TERM=linux";
const DEFAULT_HOME: &str = "HOME=/home";
const INIT_PATH: &str = "/sbin/init";
const MAX_SHEBANG_DEPTH: usize = 4;

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

fn open_file(path: Path) -> Result<Arc<FileLikeObject>, FSError> {
    Ok(Arc::new(open_path(path)?))
}

fn read_shebang_prefix(file: &FileLikeObject) -> Result<Vec<u8>, FSError> {
    let mut bytes = vec![0u8; 256];
    let read = file.read_at(&mut bytes, 0)?;
    bytes.truncate(read);
    Ok(bytes)
}

fn process_absolute_path(path: Path, fs_context: &FsContext) -> Path {
    AbsolutePath::join_under_root(
        &fs_context.root_directory,
        &fs_context.current_directory,
        &path,
    )
    .as_normal()
}

struct SetupProcessState<'a> {
    addrspace: &'a mut AddrSpace,
    fd_table: &'a mut Vec<Option<FdEntry>>,
    fs_context: &'a FsContext,
    kernel_stack_top: u64,
}

pub struct SetupProcessRequest {
    pub path: Path,
    pub exec_path: Path,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub kernel_stack_top: u64,
}

impl Process {
    pub fn init() -> ProcessRef {
        let pid = ProcessID::new();
        let addrspace = AddrSpace::default();
        let kernel_stack = allocate_owned_kernel_stack(16).finish();
        let kernel_stack_top = kernel_stack.top();

        let process_arc = Arc::new(Mut::new(Process {
            pid,
            addrspace,
            kernel_stack_top,
            group_id: ProcessGroupID::from_leader(pid),
            session_id: SessionID::from_leader(pid),
            ..Default::default()
        }));

        let process = &mut *process_arc.lock();
        let init_path = init_path();
        process.command_line = vec![init_path.clone()];
        process.exec_path = Path::new(&init_path);

        log::debug!("process {}: setup start", pid.0);
        let mut fd_table = Vec::new();
        let context = setup_process(
            SetupProcessRequest {
                path: Path::new(&init_path),
                exec_path: Path::new(&init_path),
                args: Vec::new(),
                env: alloc::vec![
                    DEFAULT_PATH.into(),
                    DEFAULT_TERM.into(),
                    DEFAULT_HOME.into()
                ],
                kernel_stack_top: kernel_stack_top.as_u64(),
            },
            &mut process.addrspace,
            &mut fd_table,
            &process.fs_context.lock().clone(),
        )
        .unwrap();
        log::debug!("process {}: setup done", pid.0);
        let new_fd_table = new_fd_table();
        *new_fd_table.lock() = fd_table;
        process.fd_table = new_fd_table;

        // Initilizes the main thread
        let main_thread = crate::thread::with_thread_manager(|manager| {
            manager.spawn(Thread::from_snapshot_with_id(
                context,
                process_arc.clone(),
                kernel_stack,
                ThreadID(pid.0),
            ))
        });
        process.threads.push(Arc::downgrade(&main_thread));

        *get_default_tty().active_group.lock() = Some(process.group_id);

        process_arc.clone()
    }
}

fn init_path() -> String {
    crate::boot::executable_cmdline()
        .and_then(|cmdline| {
            cmdline
                .split_ascii_whitespace()
                .find_map(|arg| arg.strip_prefix("init="))
        })
        .filter(|path| path.starts_with('/'))
        .unwrap_or(INIT_PATH)
        .into()
}

fn setup_process_inner(
    path: Path,
    exec_path: String,
    args: Vec<String>,
    env: Vec<String>,
    state: &mut SetupProcessState<'_>,
    shebang_depth: usize,
) -> Result<ThreadSnapshot, FSError> {
    if shebang_depth > MAX_SHEBANG_DEPTH {
        return Err(FSError::Other);
    }

    let program_file = open_file(path.clone())?;
    let program_prefix = read_shebang_prefix(&program_file)?;

    if let Some((interpreter, optional_arg)) = parse_shebang(&program_prefix)? {
        log::debug!(
            "setup_process: shebang {} -> {}",
            exec_path,
            interpreter.clone().as_string()
        );

        let mut interpreter_args = Vec::with_capacity(args.len() + 2);
        interpreter_args.push(interpreter.clone().as_string());
        if let Some(optional_arg) = optional_arg {
            interpreter_args.push(optional_arg);
        }
        interpreter_args.push(exec_path);
        interpreter_args.extend(args.into_iter().skip(1));
        let interpreter_exec_path = interpreter.clone().as_string();
        let interpreter = process_absolute_path(interpreter, state.fs_context);

        return setup_process_inner(
            interpreter,
            interpreter_exec_path,
            interpreter_args,
            env,
            state,
            shebang_depth + 1,
        );
    }

    let program_headers = read_elf_header(&program_file)?;
    let program = load_elf_lazy(state.addrspace, program_file, &program_headers)?;

    let (entry_point, interpreter_base) = match program.interpreter.as_deref() {
        Some(interpreter_path) => {
            let interp_file = open_file(Path::new(interpreter_path))?;
            let interp_headers = read_elf_header(&interp_file)?;
            let interp = load_elf_lazy(state.addrspace, interp_file, &interp_headers)?;
            prefault_exec_pages(state.addrspace, &program, Some(&interp));
            (interp.entry_point, Some(interp.load_base))
        }
        None => {
            prefault_exec_pages(state.addrspace, &program, None);
            (program.entry_point, None)
        }
    };

    let stack_pages =
        user_stack_pages_for_exec(&exec_path, &args, &env, interpreter_base.is_some());
    let mut stack_builder = state.addrspace.allocate_user_stack(stack_pages).1;

    init_stack_layout(
        &mut stack_builder,
        &program,
        interpreter_base,
        &exec_path,
        args,
        env,
    );

    init_objects(state.fd_table);

    Ok(ThreadSnapshot::new(
        entry_point,
        state.addrspace,
        stack_builder.finish().as_u64(),
        state.kernel_stack_top,
        ThreadSnapshotType::Thread,
    ))
}

fn prefault_exec_pages(
    addrspace: &mut AddrSpace,
    program: &crate::elfloader::ElfInfo,
    interpreter: Option<&crate::elfloader::ElfInfo>,
) {
    for addr in prefault_targets(program, interpreter) {
        let virt = VirtAddr::new(addr);
        let Some(area) = addrspace.get_area(virt).cloned() else {
            continue;
        };
        if !area.lazy {
            continue;
        }

        let page = Page::containing_address(virt);
        match area.data {
            crate::memory::addrspace::mem_area::Data::File { .. } => {
                addrspace.apply_page_cluster(page, area, AddrSpace::file_lazy_cluster_pages());
            }
            _ => {
                addrspace.apply_page(page, area);
            }
        }
    }
}

pub(crate) fn prefault_targets(
    program: &crate::elfloader::ElfInfo,
    interpreter: Option<&crate::elfloader::ElfInfo>,
) -> Vec<u64> {
    let mut addrs = Vec::new();
    addrs.extend(program.prefault_addrs.iter().copied());
    addrs.push(program.entry_point);
    addrs.push(program.program_header_table);

    if let Some(interpreter) = interpreter {
        addrs.extend(interpreter.prefault_addrs.iter().copied());
        addrs.push(interpreter.entry_point);
        addrs.push(interpreter.program_header_table);
    }

    addrs.sort_unstable();
    addrs.dedup();
    addrs
}

pub fn setup_process(
    request: SetupProcessRequest,
    addrspace: &mut AddrSpace,
    fd_table: &mut Vec<Option<FdEntry>>,
    fs_context: &FsContext,
) -> Result<ThreadSnapshot, FSError> {
    let exec_path = request.exec_path.as_string();
    let mut args = request.args;
    if args.is_empty() {
        args.insert(0, exec_path.clone());
    }

    let mut state = SetupProcessState {
        addrspace,
        fd_table,
        fs_context,
        kernel_stack_top: request.kernel_stack_top,
    };
    setup_process_inner(request.path, exec_path, args, request.env, &mut state, 0)
}
