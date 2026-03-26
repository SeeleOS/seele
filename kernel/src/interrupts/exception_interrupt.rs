use seele_sys::signal::Signal;
use x86_64::{
    instructions::interrupts,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

use crate::{
    interrupts::{pagefault::pagefault_handler, print_stackframe_m},
    misc::hlt_loop,
    process::manager::{MANAGER, get_current_process},
    thread::scheduling::return_to_executor_no_save,
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
    log::error!("invalid opcode");
    print_stackframe_m(stack_frame);
    hlt_loop()
}

extern "x86-interrupt" fn gp_handler(_stack_frame: InterruptStackFrame, _err_code: u64) {
    log::error!("general protection fault");
    print_stackframe_m(_stack_frame);
    hlt_loop()
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
