use core::sync::atomic::AtomicU64;

use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use elfloader::ElfBinary;
use spin::Mutex;
use x86_64::VirtAddr;

use crate::{
    memory::page_table_wrapper::PageTableWrapped,
    misc::stack_builder::StackBuilder,
    multitasking::{
        MANAGER,
        memory::{allocate_kernel_stack, allocate_stack},
        process::{
            ProcessRef,
            misc::{ProcessID, State, init_objects},
        },
        thread::{
            self, THREAD_MANAGER,
            misc::ThreadID,
            snapshot::{ThreadSnapshot, ThreadSnapshotType},
            thread::Thread,
        },
        yielding::BlockType,
    },
    object::Writable,
    s_println,
    userspace::elf_loader::load_elf,
};

#[derive(Debug)]
pub struct Process {
    pub pid: ProcessID,
    pub state: State,
    pub page_table: PageTableWrapped,
    pub kernel_stack_top: VirtAddr,
    pub threads: Vec<Weak<Mutex<Thread>>>,
    pub objects: Vec<Arc<dyn Writable>>,
}

impl Process {
    pub fn empty() -> ProcessRef {
        Arc::new(Mutex::new(Process {
            pid: ProcessID::default(),
            state: State::default(),
            page_table: PageTableWrapped::default(),
            kernel_stack_top: VirtAddr::zero(),
            threads: Vec::new(),
            objects: Vec::new(),
        }))
    }
}

impl Process {
    pub fn new(program: &[u8]) -> ProcessRef {
        let pid = ProcessID::default();
        let mut page_table = PageTableWrapped::default();
        let kernel_stack_top = allocate_kernel_stack(160, &mut page_table.inner).finish();

        let process_arc = Arc::new(Mutex::new(Process {
            pid,
            state: State::Ready,
            page_table,
            kernel_stack_top,
            threads: Vec::new(),
            objects: Vec::new(),
        }));

        let mut process = process_arc.lock();

        let mut stack_builder = allocate_stack(160, &mut process.page_table.inner);
        let program = load_elf(&mut process.page_table, program);

        assert!(!program.is_pie(), "Pie program is not supported for now");

        init_stack_layout(&mut stack_builder, &program);

        let context = ThreadSnapshot::new(
            program.entry_point() as u64,
            &mut process.page_table,
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

fn init_stack_layout(builder: &mut StackBuilder, file: &ElfBinary) {
    // A. 先在栈的最顶端存入字符串 "init\0"
    // 字符串占用 5 字节，为了对齐我们按 8 字节处理
    let arg_str = builder.push_str("init");

    // 手动移动指针存入字符串
    //*virt_stack_write = (virt_stack_write).sub(16);
    //core::ptr::copy_nonoverlapping(arg_str.as_ptr(), *virt_stack_write as *mut u8, str_len);

    builder.push(0);
    // B. 使用你的 write_and_sub 按照 ABI 逆序压栈
    builder.push_aux_entries(file);

    builder.push(0); // envp = 0

    // argv
    builder.push(0); // argv[1] == null (end)
    builder.push(arg_str); // argv[0] ==  *arg_str

    // argc (1 arguments)
    builder.push(1);
}
