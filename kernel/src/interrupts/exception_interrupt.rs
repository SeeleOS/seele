use x86_64::{
    registers::control::Cr2,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
};

use crate::{
    interrupts::print_stackframe_m,
    misc::hlt_loop,
    tss::{DOUBLE_FAULT_IST_LOCATION, GP_IST_LOCATION, PAGE_FAULT_IST_LOCATION},
};

pub fn init_exception_interrupts(idt: &mut InterruptDescriptorTable) {
    idt.breakpoint.set_handler_fn(breakpoint_handler);
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

extern "x86-interrupt" fn gp_handler(_stack_frame: InterruptStackFrame, _err_code: u64) {
    log::error!("general protection fault");
    print_stackframe_m(_stack_frame);
    hlt_loop()
}

extern "x86-interrupt" fn double_fault_handler(
    _stack_frame: InterruptStackFrame,
    err_code: u64,
) -> ! {
    panic!(
        "Double fault:\n\n{:#?}\nError code: {err_code}",
        _stack_frame
    );
}

// i gave up on trying to wrap everything behind a abstraction layer.
extern "x86-interrupt" fn pagefault_handler(
    _stack_frame: InterruptStackFrame,
    err_code: PageFaultErrorCode,
) {
    log::error!("Page fault");
    log::error!("Address: {:?}", Cr2::read());
    log::error!("Error code: {:?}", err_code);
    print_stackframe_m(_stack_frame);
    hlt_loop();
}
