use core::{arch::naked_asm, mem::offset_of};

use x86_64::{
    VirtAddr,
    instructions::interrupts,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

use crate::{
    interrupts::pagefault::pagefault_user_wrapper,
    misc::{
        snapshot::{Snapshot, SnapshotWithErrorCode},
        tss::*,
    },
    process::{
        ProcessExitStatus,
        manager::{get_current_process, terminate_process},
    },
    signal::{Signal, process_current_process_signals, send_signal_to_process},
    smp::gs::GsContext,
    thread::{misc::with_current_thread, scheduling::return_to_scheduler_no_save},
};

pub fn init_exception_interrupts(idt: &mut InterruptDescriptorTable) {
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    unsafe {
        idt.invalid_opcode.set_handler_addr(VirtAddr::new(
            invalid_opcode_user_wrapper as *const () as u64,
        ));
    }
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(DOUBLE_FAULT_IST_LOCATION);
        idt.page_fault
            .set_handler_addr(VirtAddr::new(pagefault_user_wrapper as *const () as u64))
            .set_stack_index(PAGE_FAULT_IST_LOCATION);
        idt.general_protection_fault
            .set_handler_addr(VirtAddr::new(gp_user_wrapper as *const () as u64))
            .set_stack_index(GP_IST_LOCATION);
    }
}

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {}

extern "C" fn invalid_opcode_handler(snapshot: &Snapshot, from_user: u64) -> ! {
    if from_user != 0 {
        handle_usermode_exception(snapshot, Signal::SIGILL);
    }

    #[cfg(not(test))]
    panic!("invalid opcode.\n {snapshot:#?}");

    #[cfg(test)]
    unreachable!()
}

extern "C" fn gp_handler(snapshot: &SnapshotWithErrorCode, _err_code: u64, from_user: u64) -> ! {
    let snapshot = snapshot.as_snapshot();
    if from_user != 0 {
        handle_usermode_exception(&snapshot, Signal::SIGSEGV);
    }

    panic!("GP fault. \n {snapshot:#?}");
}

extern "x86-interrupt" fn double_fault_handler(
    _stack_frame: InterruptStackFrame,
    err_code: u64,
) -> ! {
    interrupts::disable();
    panic!(
        "Double fault:\n\n{:#?}\nError code: {err_code}",
        _stack_frame
    );
}

#[unsafe(naked)]
extern "C" fn invalid_opcode_user_wrapper() {
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
        "push r15",
        "xor esi, esi",
        "test qword ptr [rsp + {CS_OFF}], 0x3",
        "setnz sil",
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
        "call {inner}",
        "ud2",
        inner = sym invalid_opcode_handler,
        CS_OFF = const offset_of!(Snapshot, cs),
        ACTIVE_EXT_STATE_OFF = const offset_of!(GsContext, active_user_extended_state),
        ACTIVE_EXT_STATE_SAVED_OFF =
            const offset_of!(GsContext, active_user_extended_state_saved),
        USES_XSAVE_OFF = const offset_of!(GsContext, extended_state_uses_xsave),
        XCR0_LOW_OFF = const offset_of!(GsContext, extended_state_xcr0),
        XCR0_HIGH_OFF = const offset_of!(GsContext, extended_state_xcr0) + 4,
    )
}

#[unsafe(naked)]
extern "C" fn gp_user_wrapper() {
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
        "push r15",
        "xor edx, edx",
        "test qword ptr [rsp + {CS_OFF}], 0x3",
        "setnz dl",
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
        "mov rsi, [rsp + {ERR_OFF}]",
        "call {inner}",
        "ud2",
        inner = sym gp_handler,
        ERR_OFF = const offset_of!(SnapshotWithErrorCode, error_code),
        CS_OFF = const offset_of!(SnapshotWithErrorCode, cs),
        ACTIVE_EXT_STATE_OFF = const offset_of!(GsContext, active_user_extended_state),
        ACTIVE_EXT_STATE_SAVED_OFF =
            const offset_of!(GsContext, active_user_extended_state_saved),
        USES_XSAVE_OFF = const offset_of!(GsContext, extended_state_uses_xsave),
        XCR0_LOW_OFF = const offset_of!(GsContext, extended_state_xcr0),
        XCR0_HIGH_OFF = const offset_of!(GsContext, extended_state_xcr0) + 4,
    )
}

pub fn handle_usermode_exception(snapshot: &Snapshot, sig: Signal) -> ! {
    // Save the current user context manually because exception entry did not
    // pass through the normal syscall snapshot path.
    with_current_thread(|thread| {
        let fs_base = {
            let thread_snapshot = thread.get_appropriate_snapshot();
            thread_snapshot.inner = *snapshot;
            thread_snapshot.fs_base
        };
        thread.last_user_snapshot = *snapshot;
        thread.last_user_fs_base = fs_base;
    });

    let process = get_current_process();
    send_signal_to_process(&process, sig);
    let should_switch = process_current_process_signals(&process);

    if should_switch {
        crate::thread::with_thread_manager(|manager| manager.cleanup_exited_threads());
        return_to_scheduler_no_save();
    }

    terminate_process(get_current_process(), ProcessExitStatus::Signaled(sig));
    return_to_scheduler_no_save();
}
