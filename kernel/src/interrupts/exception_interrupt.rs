use x86_64::{
    instructions::interrupts,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

use crate::{
    interrupts::pagefault::pagefault_handler,
    misc::{others::is_user_mode, tss::*},
    process::{
        ProcessExitStatus,
        manager::{get_current_process, terminate_process},
    },
    signal::{Signal, process_current_process_signals, send_signal_to_process},
    thread::{THREAD_MANAGER, misc::with_current_thread, scheduling::return_to_scheduler_no_save},
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
        handle_usermode_exception(&stack_frame, Signal::SIGILL);
    }

    panic!("invalid opcode.\n {:#?}", stack_frame);
}

extern "x86-interrupt" fn gp_handler(stack_frame: InterruptStackFrame, _err_code: u64) {
    if is_user_mode(&stack_frame) {
        handle_usermode_exception(&stack_frame, Signal::SIGSEGV);
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
    log_boot_debug_exception(stackframe, sig);

    // Save the state of the current thread manually with the stackframe.
    // We need to do this because the snapshot wont
    // get automatically saved, unlike in syscalls.
    with_current_thread(|thread| {
        thread
            .get_appropriate_snapshot()
            .inner
            .update_with_stackframe(stackframe);
    });

    let process = get_current_process();
    send_signal_to_process(&process, sig);
    let should_switch = process_current_process_signals(&process);

    if should_switch {
        THREAD_MANAGER
            .get()
            .unwrap()
            .lock()
            .cleanup_exited_threads();
        return_to_scheduler_no_save();
    }

    terminate_process(get_current_process(), ProcessExitStatus::Signaled(sig));
    return_to_scheduler_no_save();
}

fn log_boot_debug_exception(stackframe: &InterruptStackFrame, sig: Signal) {
    crate::process::misc::with_current_process(|process| {
        let Some(command) = process.command_line.first() else {
            return;
        };
        let Some(name) = command.rsplit('/').next() else {
            return;
        };
        if !matches!(name, "init" | "systemd" | "systemd-random-seed" | "systemd-sysusers") {
            return;
        }

        let tid = crate::thread::get_current_thread().lock().id.0;
        crate::s_println!(
            "bootexc sig={:?} comm={} pid={} tid={} rip={:#x} rsp={:#x}",
            sig,
            name,
            process.pid.0,
            tid,
            stackframe.instruction_pointer.as_u64(),
            stackframe.stack_pointer.as_u64()
        );
    });
}
