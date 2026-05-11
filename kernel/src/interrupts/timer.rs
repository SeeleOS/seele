use core::{arch::naked_asm, mem::offset_of};

use crate::{
    interrupts::hardware_interrupt::send_eoi,
    misc::snapshot::Snapshot,
    smp::gs::GsContext,
    thread::{scheduling::return_to_scheduler, snapshot::ThreadSnapshotType},
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
        "test qword ptr [rsp + {CS_OFF}], 0x3",
        "jz 0f",
        "swapgs",
        "mov r8, qword ptr gs:[{ACTIVE_EXT_STATE_OFF}]",
        "test r8, r8",
        "jz 0f",
        "cmp qword ptr gs:[{ACTIVE_EXT_STATE_SAVED_OFF}], 0",
        "jne 0f",
        "cmp qword ptr gs:[{USES_XSAVE_OFF}], 0",
        "je 1f",
        "mov eax, dword ptr gs:[{XCR0_LOW_OFF}]",
        "mov edx, dword ptr gs:[{XCR0_HIGH_OFF}]",
        "xsave64 [r8]",
        "jmp 2f",
        "1:",
        "fxsave64 [r8]",
        "2:",
        "mov qword ptr gs:[{ACTIVE_EXT_STATE_SAVED_OFF}], 1",
        "0:",
        "mov rdi, rsp",
        "call {handler}",
        "test qword ptr [rsp + {CS_OFF}], 0x3",
        "jz 5f",
        "mov r8, qword ptr gs:[{ACTIVE_EXT_STATE_OFF}]",
        "test r8, r8",
        "jz 4f",
        "cmp qword ptr gs:[{USES_XSAVE_OFF}], 0",
        "je 3f",
        "mov eax, dword ptr gs:[{XCR0_LOW_OFF}]",
        "mov edx, dword ptr gs:[{XCR0_HIGH_OFF}]",
        "xrstor64 [r8]",
        "jmp 4f",
        "3:",
        "fxrstor64 [r8]",
        "4:",
        "mov qword ptr gs:[{ACTIVE_EXT_STATE_SAVED_OFF}], 0",
        "swapgs",
        "5:",
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
        CS_OFF = const offset_of!(Snapshot, cs),
        ACTIVE_EXT_STATE_OFF = const offset_of!(GsContext, active_user_extended_state),
        ACTIVE_EXT_STATE_SAVED_OFF =
            const offset_of!(GsContext, active_user_extended_state_saved),
        USES_XSAVE_OFF = const offset_of!(GsContext, extended_state_uses_xsave),
        XCR0_LOW_OFF = const offset_of!(GsContext, extended_state_xcr0),
        XCR0_HIGH_OFF = const offset_of!(GsContext, extended_state_xcr0) + 4,
    )
}

pub extern "C" fn timer_interrupt_handler(snapshot: &mut Snapshot) {
    send_eoi();

    // Don't preempt kernel mode; it can corrupt in-flight kernel snapshots.
    if (snapshot.cs & 0x3) == 0 {
        return;
    }

    return_to_scheduler(snapshot, ThreadSnapshotType::Thread);

    unreachable!();
}
