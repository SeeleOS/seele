use x86_64::{
    PrivilegeLevel,
    instructions::interrupts,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

use crate::{
    interrupts::{pagefault::pagefault_handler, print_stackframe_m},
    misc::{hlt_loop, others::is_user_mode},
    process::{
        manager::{MANAGER, get_current_process, terminate_process},
        misc::with_current_process,
    },
    s_println,
    signal::Signal,
    thread::{misc::with_current_thread, scheduling::return_to_executor_no_save},
    tss::{DOUBLE_FAULT_IST_LOCATION, GP_IST_LOCATION, PAGE_FAULT_IST_LOCATION},
};

pub fn init_exception_interrupts(idt: &mut InterruptDescriptorTable) {
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(DOUBLE_FAULT_IST_LOCATION);
        idt.page_fault
            .set_handler_fn(pagefault_handler)
            .set_stack_index(PAGE_FAULT_IST_LOCATION);
        idt.general_protection_fault
            .set_handler_fn(gp_handler)
            .set_stack_index(GP_IST_LOCATION);
    }
}

extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    if is_user_mode(&stack_frame) {
        handle_usermode_exception(&stack_frame, Signal::IllegalInstruction);
    }

    panic!("invalid opcode.\n {:#?}", stack_frame);
}

extern "x86-interrupt" fn gp_handler(stack_frame: InterruptStackFrame, _err_code: u64) {
    if is_user_mode(&stack_frame) {
        handle_usermode_exception(&stack_frame, Signal::InvalidMemoryAccess);
    }

    panic!("GP fault. \n {:#?}", stack_frame);
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

pub fn handle_usermode_exception(stackframe: &InterruptStackFrame, sig: Signal) -> ! {
    // Save the state of the current thread manually with the stackframe.
    // We need to do this because the snapshot wont
    // get automatically saved, unlike in syscalls.
    with_current_thread(|thread| {
        thread
            .get_appropriate_snapshot()
            .inner
            .update_with_stackframe(stackframe);
    });

    let should_switch = with_current_process(|process| {
        process.send_signal(sig);
        process.process_signals()
    });

    if should_switch {
        return_to_executor_no_save();
    }

    terminate_process(get_current_process(), sig as u64);
    return_to_executor_no_save();
}
