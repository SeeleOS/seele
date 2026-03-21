use core::arch::naked_asm;

use crate::{
    interrupts::hardware_interrupt::{HardwareInterrupt, send_eoi},
    misc::{snapshot::Snapshot, with_cpu_core_context},
    multitasking::{scheduling::return_to_executor, thread::snapshot::ThreadSnapshotType},
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
        // If the handler returns, restore registers and iretq.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rbp",
        "pop rbx",
        "pop rdx",
        "pop rcx",
        "pop rax",
        "iretq",
        handler = sym timer_interrupt_handler, // 符号绑定
    )
}

pub extern "C" fn timer_interrupt_handler(snapshot: &mut Snapshot) {
    send_eoi();
    // Don't preempt kernel mode; it can corrupt in-flight kernel snapshots.
    if (snapshot.cs & 0x3) == 0 {
        return;
    }

    return_to_executor(snapshot, ThreadSnapshotType::Thread);

    panic!("What the fuck");
}
