use core::iter::empty;

use x86_64::{VirtAddr, registers::control::Cr3Flags};

use crate::{
    gdt::GDT,
    memory::page_table_wrapper::PageTableWrapped,
    misc::misc::calc_cr3_value,
    multitasking::memory::{allocate_kernel_stack, allocate_stack},
    userspace::elf_loader::Function,
};

// NOTE: the direction of the struct in memory and the stack is REVERSED
// therefore you need to push rbp - r15 and then rflags
// and also, ptr.sub(1) 6 times (rbp-r15) and then write the rflags
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Context {
    cr3: u64, // +0
    // RSP used on context switching in kernel space to not messup the userstack
    pub kernel_rsp: u64, // +8

    r15: u64, // +16
    r14: u64, // +24
    r13: u64, // +32
    r12: u64, // +40
    rbx: u64, // +48
    rbp: u64, // +56

    pub ss: u64, // +64
    // The actural RSP when the program is running
    pub user_rsp: u64, // +72
    pub rflags: u64,   // +80
    pub cs: u64,       // +88
    pub rip: u64,      // +96

    pub fs_base: u64,
}

impl Context {
    pub fn new(entry_point: u64, table: &mut PageTableWrapped, virt_stack_addr: u64) -> Self {
        Self {
            cr3: calc_cr3_value(table.frame.start_address(), Cr3Flags::empty()),
            kernel_rsp: allocate_kernel_stack(16, &mut table.inner)
                .finish()
                .as_u64(),

            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,

            ss: GDT.1.user_data.0 as u64,
            user_rsp: virt_stack_addr,
            //rflags: 0x202, TODO: enable it
            rflags: 0x0,
            cs: GDT.1.user_code.0 as u64,
            rip: entry_point,

            fs_base: 0,
        }
    }

    pub fn as_ptr(&mut self) -> *mut Self {
        self as *mut Self
    }
}
