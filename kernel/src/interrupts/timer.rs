use core::arch::naked_asm;

use crate::{
    interrupts::hardware_interrupt::{HardwareInterrupt, notify_end_of_interrupt},
    misc::snapshot::Snapshot,
    multitasking::scheduling::return_to_executor,
    s_println,
};

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn timer_interrupt_handler_wrapper() {
    naked_asm!(
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
            "push r15", // 它是最后一个入栈的，地址最低
        "mov rdi, rsp",
        "call {handler}",
        handler = sym timer_interrupt_handler, // 符号绑定
    )
}

pub extern "C" fn timer_interrupt_handler(snapshot: &mut Snapshot) {
    notify_end_of_interrupt(HardwareInterrupt::Timer);
    return_to_executor(snapshot);

    panic!("What the fuck");
}
