use core::arch::naked_asm;

use x86_64::structures::idt::InterruptStackFrame;

use crate::{misc::snapshot::Snapshot, multitasking::scheduling::run_next, s_println};

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn timer_interrupt_handler_wrapper() {
    naked_asm!(
        // Saves all register to the snapshot
        "push rax",
        "push rcx",
        "push rdx",
        "push rbx",
        "push rbp",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        "mov rdi, rsp",
        "call {handler}",
        handler = sym timer_interrupt_handler, // 符号绑定
    )
}

pub extern "C" fn timer_interrupt_handler(snapshot: &mut Snapshot) {
    s_println!("timer");
    run_next(snapshot);

    panic!("What the fuck");
}
