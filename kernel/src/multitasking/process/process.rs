use core::sync::atomic::AtomicU64;

use alloc::boxed::Box;
use elfloader::ElfBinary;
use x86_64::{
    VirtAddr,
    registers::model_specific::Msr,
    structures::paging::{Mapper, Page, Size4KiB, Translate},
};

use crate::{
    memory::page_table_wrapper::PageTableWrapped,
    misc::{aux::AuxType, stack_builder::StackBuilder},
    multitasking::{
        memory::{allocate_kernel_stack, allocate_stack},
        process::context::ProcessSnapshot,
        yielding::BlockType,
    },
    s_println,
    userspace::elf_loader::{Function, load_elf},
};

#[derive(Debug)]
pub struct Process {
    pub pid: ProcessID,
    pub context: ProcessSnapshot,
    pub state: State,
    pub page_table: PageTableWrapped,
    pub kernel_stack_top: VirtAddr,
}

// TODO: add threads, and make process just a wrapper/container of threads
impl Default for Process {
    fn default() -> Self {
        Self {
            page_table: PageTableWrapped::default(),
            pid: ProcessID::default(),
            context: ProcessSnapshot::default(),
            state: State::Ready,
            kernel_stack_top: VirtAddr::zero(),
        }
    }
}

impl Process {
    pub fn new(program: &[u8]) -> Self {
        let mut page_table = PageTableWrapped::default();

        let mut stack_builder = allocate_stack(160, &mut page_table.inner);

        let program = load_elf(&mut page_table, program);

        assert!(!program.is_pie(), "Pie program is not supported for now");

        init_stack_layout(&mut stack_builder, &program);

        let context = ProcessSnapshot::new(
            program.entry_point() as u64,
            &mut page_table,
            stack_builder.finish().as_u64(),
        );
        let kernel_stack_top = allocate_kernel_stack(160, &mut page_table.inner).finish();

        Self {
            page_table,
            pid: ProcessID::default(),
            context,
            state: State::Ready,
            kernel_stack_top,
        }
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessID(pub u64);

impl Default for ProcessID {
    fn default() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        Self(NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum State {
    Ready, // ready to run (in a queue)
    Running,
    Blocked(BlockType), // stuck, waiting for something (like keyboard input)
    Zombie,             // Exited process
}
